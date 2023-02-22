use crate::dep_groups::strategy_trta::StrategyTRTA;
use crate::dep_groups::utils::*;
use crate::hard_policies::{Condition, HardPolicy};
use crate::netsim::config::{ConfigExpr, ConfigModifier};
use crate::netsim::types::Destination::*;
use crate::netsim::{BgpSessionType, ForwardingState, Network, NetworkError, Prefix, RouterId};
use crate::parallelism::{ConfigId, Dag, SolutionBuilder};
use crate::strategies::{Strategy, StrategyDAG};
use crate::{Error, Stopper};
use std::collections::{HashMap, HashSet};
use std::iter::zip;
use std::time::{Duration, Instant};

pub(crate) type ZoneId = RouterId;

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: ZoneId,                 // Identifier of the current zone
    pub routers: HashSet<RouterId>, // Routers that belong to this zone
    pub configs: Vec<ConfigId>,     // Configurations that belong to this zone
    ordering: Option<Vec<ConfigModifier>>,
    hard_policy: HashSet<Condition>,
    virtual_boundary_routers: HashSet<RouterId>,
    emulated_network: Network,
}

impl Zone {
    pub fn new(id: RouterId, net: &Network) -> Self {
        Self {
            id: id,
            routers: HashSet::new(),
            configs: Vec::new(),
            ordering: None,
            hard_policy: HashSet::new(),
            virtual_boundary_routers: HashSet::new(),
            emulated_network: net.to_owned(),
        }
    }

    fn contains_router(&self, router: &RouterId) -> bool {
        self.routers.contains(router)
    }

    fn set_routers(&mut self, routers: Vec<RouterId>) {
        routers.iter().for_each(|r| self.add_router(*r));
    }

    fn add_router(&mut self, router: RouterId) {
        self.routers.insert(router);
    }

    fn set_emulated_network(
        &mut self,
        before_state: &mut ForwardingState,
        after_state: &mut ForwardingState,
    ) -> Result<(), NetworkError> {
        // println!("Zone: {:?}", self.routers);
        // Step 1: Identify and initialize virtual boundary routers
        self.init_virtual_boundary_routers()?;

        // Do we really want this type of incomplete virtual mapping?

        // Step 2: Iterate through all virtual routers in the set and check for every prefix whether the next hop is outside of the zone.
        // If either of the next hop is outside of the zone, we add a virtual link to the corresponding router,
        // else we don't add any link
        for virtual_boundary_router in self.get_virtual_boundary_routers() {
            let mut virtual_links = HashMap::new();
            for prefix in self.emulated_network.get_known_prefixes() {
                let before_route =
                    before_state.get_route_new(virtual_boundary_router, BGP(*prefix)).unwrap();
                // println!("Before: {:?}", before_route);
                if !self.get_routers().contains(&before_route[1]) {
                    // println!("{:?}")
                    let ptr = virtual_links.entry(before_route[1]).or_insert(HashSet::new());
                    ptr.insert(*before_route.last().unwrap());
                }
                let after_route =
                    after_state.get_route_new(virtual_boundary_router, BGP(*prefix)).unwrap();
                // println!("After: {:?}", after_route);
                if !self.get_routers().contains(&after_route[1]) {
                    let ptr = virtual_links.entry(after_route[1]).or_insert(HashSet::new());
                    ptr.insert(*after_route.last().unwrap());
                }
            }
            // println!("Virtual links: {:?}", &virtual_links);
            self.emulated_network
                .set_virtual_links_for_router(virtual_boundary_router, virtual_links)?;
        }
        Ok(())
    }

    fn init_virtual_boundary_routers(&mut self) -> Result<(), NetworkError> {
        self.virtual_boundary_routers = self
            .emulated_network
            .init_zone(&self.routers.iter().map(|x| *x).collect())?
            .into_iter()
            .collect();
        Ok(())
    }

    fn add_config(&mut self, config: usize) {
        self.configs.push(config);
    }

    fn add_hard_policy(&mut self, condition: Condition) {
        if !self.hard_policy.contains(&condition) {
            self.hard_policy.insert(condition);
        }
    }

    fn _get_id(&self) -> RouterId {
        return self.id;
    }

    fn get_routers(&self) -> HashSet<RouterId> {
        self.routers.clone()
    }

    fn get_configs(&self) -> Vec<usize> {
        self.configs.clone()
    }

    fn get_ordering(&self) -> Option<Vec<ConfigModifier>> {
        self.ordering.clone()
    }

    fn get_virtual_boundary_routers(&self) -> HashSet<RouterId> {
        self.virtual_boundary_routers.clone().into_iter().collect()
    }

    fn get_policy(&self) -> Vec<Condition> {
        self.hard_policy.clone().into_iter().collect()
    }

    fn get_emulated_network(&self) -> &Network {
        &self.emulated_network
    }

    // This function takes in an array of configurations, filter out the irrelevant configurations,
    // pass the relevant configs into strartegy_trta, map the configs output to their ids
    fn solve_ordering(&mut self, configs: &Vec<ConfigModifier>) -> Result<Duration, Error> {
        let relevant_configs = self.map_idx_to_config(configs)?;
        // Convert the conditions into hard policy object?
        let start_time = Instant::now();
        let mut strategy = StrategyTRTA::new(
            self.get_emulated_network().to_owned(),
            relevant_configs.clone(),
            HardPolicy::globally(self.get_policy()),
            None,
        )?;
        self.ordering = Some(strategy.work(Stopper::new())?);
        // println!("{:?}", self.ordering);
        let end_time = start_time.elapsed();
        // self.emulated_network.undo_action();
        Ok(end_time)
    }

    fn map_idx_to_config(
        &self,
        configs: &Vec<ConfigModifier>,
    ) -> Result<Vec<ConfigModifier>, Error> {
        let mut relevant_configs = Vec::new();
        for i in self.get_configs() {
            if configs.len() <= i {
                return Err(Error::ZoneSegmentationFailed);
            }
            relevant_configs.push(configs[i].to_owned());
        }
        Ok(relevant_configs)
    }
}

pub struct StrategyZone {
    net: Network,
    zones: HashMap<ZoneId, Zone>,
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    before_state: ForwardingState,
    after_state: ForwardingState,
    _stop_time: Option<Duration>,
}

impl StrategyDAG for StrategyZone {
    fn new(
        net: Network,
        modifiers: Vec<ConfigModifier>,
        hard_policy: HardPolicy,
        time_budget: Option<Duration>,
    ) -> Result<Box<Self>, crate::Error> {
        let before_state = net.get_forwarding_state_new();
        let after_state = StrategyZone::get_after_states(&net, &modifiers).unwrap();
        Ok(Box::new(Self {
            net,
            zones: HashMap::<ZoneId, Zone>::new(),
            modifiers,
            hard_policy,
            before_state,
            after_state,
            _stop_time: time_budget,
        }))
    }

    fn work(&mut self, _abort: Stopper) -> Result<Dag<ConfigId>, Error> {
        let mut builder = SolutionBuilder::new();

        let start_time = Instant::now();
        self.init_zones()?;

        println!("Partitioned into {} zones", self.zones.len());

        let router_to_zone = zone_into_map(&self.zones);

        // First verify if the hard policy is global reachability
        // The current algorithm cannot handle other LTL modals
        if !self.hard_policy.is_global_reachability() {
            return Err(Error::NotImplemented);
        }
        let condition = self.hard_policy.prop_vars.clone();

        let premliminary_duration = start_time.elapsed();

        let mut split_invariance_duration = Duration::new(0, 0);
        for c in &condition {
            let split_invariance_start = Instant::now();
            match c {
                // only support reachable/not reachable condtions
                Condition::Reachable(r, p, _) => {
                    println!("Splitting reachable condition");
                    let before_path = self.before_state.get_route_new(*r, BGP(*p)).unwrap();
                    let after_path = self.after_state.get_route_new(*r, BGP(*p)).unwrap();
                    // also need to retrieve the AS info of this router
                    // the last hop must end at an external router
                    // segment routers on new route and old route into different zones
                    let before_zones = segment_path(&router_to_zone, &before_path)?;
                    let after_zones = segment_path(&router_to_zone, &after_path)?;
                    println!("Before zones: {:?}", before_zones);
                    println!("After zones: {:?}", after_zones);
                    // create propositional variables and virtual boundaries using these zones
                    self.split_invariance_add_to_zones(p, &before_zones, &router_to_zone)?;
                    self.split_invariance_add_to_zones(p, &after_zones, &router_to_zone)?;

                    // Set up node dependency here
                    // Indicator 1: the router is in a different zone from the initial router
                    // Indicator 2: the router has a different next hop in the new state

                    // To satisfy indicator 1, we skip nodes that are in the same zone as router r
                    for i in 1..before_zones.len() {
                        for s in &before_zones[i] {
                            // Check if the router has a different next hop in the new state
                            if self.before_state.has_diff_next_hop(*s, *p, &self.after_state) {
                                builder.add_node_dependency(*r, *s)?;
                                println!("Adding dependency");
                            }
                        }
                    }
                    for i in 1..after_zones.len() {
                        for s in &after_zones[i] {
                            if self.before_state.has_diff_next_hop(*s, *p, &self.after_state) {
                                builder.add_node_dependency(*s, *r)?;
                            }
                        }
                    }
                }
                // Might need adaptation for condition NotReachable
                _ => {
                    return Err(Error::NotImplemented);
                }
            }
            let split_invariance_end = split_invariance_start.elapsed();
            if split_invariance_end > split_invariance_duration {
                split_invariance_duration = split_invariance_end;
            }
        }

        println!("Node dependency: {:?}", builder.get_node_dependency());

        let solve_duration = self.solve_zone_orderings()?;

        let dag_construct_start = Instant::now();
        self.assemble_zone_orderings(&mut builder)?;
        let dag_construct_duration = dag_construct_start.elapsed();

        println!("Preliminary: {:?}", premliminary_duration);
        println!("Split invariance: {:?}", split_invariance_duration);
        println!("Solve: {:?}", solve_duration);
        println!("Dag construct: {:?}", dag_construct_duration);
        let total_exec_time = premliminary_duration
            + split_invariance_duration
            + solve_duration
            + dag_construct_duration;
        println!("Total time: {:?}", total_exec_time);
        Ok(builder.get_config_dependency().to_owned())
    }
}

impl StrategyZone {
    fn init_zones(&mut self) -> Result<(), Error> {
        let mut zones = self.partition_zone();
        self.bind_config_to_zone(&mut zones);
        // Iterate over each zone identified
        for zone in zones.values_mut() {
            zone.set_emulated_network(&mut self.before_state, &mut self.after_state)?;
        }
        self.zones = zones;
        Ok(())
    }

    fn solve_zone_orderings(&mut self) -> Result<Duration, Error> {
        let mut max_time = Duration::new(0, 0);
        for z in self.zones.values_mut() {
            let time = z.solve_ordering(&self.modifiers)?;
            if time > max_time {
                max_time = time;
            }
        }
        println!("Max time for solving zone is: {:?}", max_time);
        Ok(max_time)
    }

    fn assemble_zone_orderings(&self, builder: &mut SolutionBuilder) -> Result<(), Error> {
        let config_to_idx_map: HashMap<_, _> =
            self.modifiers.iter().enumerate().map(|(x, y)| (y.key(), x)).collect();
        for z in self.zones.values() {
            match z.get_ordering() {
                Some(config_ordering) => {
                    let idx_ordering: Vec<_> = config_ordering
                        .iter()
                        .map(|x| *config_to_idx_map.get(&x.key()).unwrap())
                        .collect();
                    let mut idx_set = HashSet::new();
                    for i in idx_ordering.iter() {
                        if !idx_set.insert(*i) {
                            println!("Duplicate");
                        }
                    }
                    // create timestamps that correspond to configuration
                    let time_stamps = self.net.clone().apply_modifiers_check_next_hop(
                        &z.get_routers().into_iter().collect(),
                        &config_ordering,
                    )?;
                    // we also need to compute the timestamp at each step
                    builder
                        .insert_config_ordering(&zip(idx_ordering, time_stamps).collect())
                        .unwrap();
                }
                // Need to add errors that cater to DAG and parallel executor
                None => return Err(Error::ZoneSegmentationFailed),
            }
        }
        builder.construct_config_dependency().unwrap();
        Ok(())
    }

    fn partition_zone(&self) -> HashMap<RouterId, Zone> {
        let net = &self.net;
        let mut zones = HashMap::<RouterId, Zone>::new();
        let router_ids = net.get_routers();
        'outer: for id in &router_ids {
            let router = net.get_device(*id).unwrap_internal();

            // Determine if the current router can no longer propagate the advertisement
            '_inner: for (peer_id, session_type) in &router.bgp_sessions {
                match *session_type {
                    // Only the router that is not a route reflector nor a boundary router can be added
                    BgpSessionType::EBgp => continue 'outer,
                    BgpSessionType::IBgpPeer => {
                        // if my peer router is my iBGP client, then I am a route reflector
                        // thus cannot be in a zone
                        if self.is_self_client(id, peer_id) {
                            continue 'outer;
                        }
                    }
                    _ => {}
                }
            }

            let mut zone = Zone::new(*id, net);
            let router_set = bgp_zone_extractor(*id, net).unwrap();
            zone.set_routers(router_set.into_iter().collect());
            // push the current zone into the final collection of zones
            zones.insert(*id, zone);
        }
        // self.bind_config_to_zone(&mut zones);
        // then we construct a virtual boundary by adding symbolic links between virtual boundary routers and external routers
        zones
    }

    fn bind_config_to_zone(&self, zones: &mut HashMap<RouterId, Zone>) {
        for (_, z) in zones {
            for (idx, config) in self.modifiers.iter().enumerate() {
                match config {
                    ConfigModifier::Insert(c) | ConfigModifier::Remove(c) => {
                        // Only implement zone partitioning for BGP Session configurations
                        if let ConfigExpr::BgpSession { source, target, session_type } = c {
                            // Test if both routers are in zone
                            match (z.contains_router(source), z.contains_router(target)) {
                                // both routers are in the zone
                                (true, true) => z.add_config(idx),
                                (true, false) => {
                                    // if the source router is in the zone and is a client, or is an eBGP
                                    if *session_type == BgpSessionType::IBgpClient
                                        || *session_type == BgpSessionType::EBgp
                                    {
                                        z.add_config(idx);
                                    } else {
                                        // or the target router is a route reflector/boundary router
                                        if self.is_client_or_boundary(target)
                                            && !self.is_self_client(source, target)
                                        {
                                            z.add_config(idx);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else if let ConfigExpr::BgpRouteMap { router, direction: _, map: _ } = c {
                            if z.contains_router(router) {
                                z.add_config(idx);
                            }
                        }
                    }
                    ConfigModifier::Update { from: c1, to: c2 } => match (c1, c2) {
                        (
                            ConfigExpr::BgpSession {
                                source: s1,
                                target: t1,
                                session_type: session1,
                            },
                            ConfigExpr::BgpSession { source: _, target: _, session_type: session2 },
                        ) => {
                            let routers_in_zone = (z.contains_router(s1), z.contains_router(t1));
                            match routers_in_zone {
                                (true, true) => z.add_config(idx),
                                (true, false)
                                    if *session1 == BgpSessionType::IBgpClient
                                        || *session2 == BgpSessionType::IBgpClient =>
                                {
                                    z.add_config(idx);
                                }
                                _ => {}
                            }
                        }
                        (
                            ConfigExpr::BgpRouteMap { router, direction: _, map: _ },
                            ConfigExpr::BgpRouteMap { router: _, direction: _, map: _ },
                        ) => {
                            if z.contains_router(router) {
                                z.add_config(idx);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        // zones
    }

    fn split_invariance_add_to_zones(
        &mut self,
        p: &Prefix,
        segmented_paths: &Vec<Vec<RouterId>>,
        router_to_zone: &HashMap<RouterId, Vec<ZoneId>>,
    ) -> Result<(), Error> {
        let _external_router = segmented_paths.last().unwrap().last().unwrap();
        for x in segmented_paths {
            let router = x.first().unwrap();
            let virtual_boundary = x.last().unwrap();
            match (router_to_zone.get(router), router_to_zone.get(virtual_boundary)) {
                // skip the cases where any one of them belong to different zones
                (Some(zone1), Some(zone2)) => {
                    let set1: HashSet<_> = zone1.into_iter().collect();
                    let set2: HashSet<_> = zone2.into_iter().collect();
                    // calculate the intersection of the zones the beginning and the ending routers belong to
                    let intersect = set1.intersection(&set2);
                    // only add policy, virtual boundary router to the zones that are common
                    // println!("Zone intersection: {:?}", intersect);
                    for z in intersect {
                        if let Some(zone_ptr) = self.zones.get_mut(*z) {
                            zone_ptr.add_hard_policy(Condition::Reachable(*router, *p, None));
                        }
                    }
                }
                _ => continue,
            }
        }
        Ok(())
    }

    fn get_after_states(
        net: &Network,
        modifier_vector: &Vec<ConfigModifier>,
    ) -> Result<ForwardingState, Error> {
        let mut after_net = net.to_owned();
        after_net.apply_modifier_vector(modifier_vector)?;
        Ok(after_net.get_forwarding_state_new())
    }

    fn is_client_or_boundary(&self, rid: &RouterId) -> bool {
        let router = self.net.get_device(*rid).unwrap_internal();
        let result = router
            .bgp_sessions
            .iter()
            .map(|(_, session)| {
                (*session == BgpSessionType::EBgp) || (*session == BgpSessionType::IBgpClient)
            })
            .fold(false, |acc, x| (acc | x));
        result
    }

    fn is_self_client(&self, self_id: &RouterId, other_id: &RouterId) -> bool {
        // this function is somewhat problematic
        // we can use a better way to rewrite it (with cache)
        let other_router = self.net.get_device(*other_id).unwrap_internal();
        if !other_router.bgp_sessions.contains_key(self_id) {
            return false;
        }
        other_router.bgp_sessions[self_id] == BgpSessionType::IBgpClient
    }
}

/// For each non-route-reflector internal device, find the zone it belongs to
/// returned in a HashMap, key is the internal router's id, value is the associated zone
// In the future can be modified to any router that maintains reachability conditions

#[cfg(test)]
mod test {
    // use crate::dep_groups::strategy_trta::StrategyTRTA;
    use crate::dep_groups::strategy_zone::StrategyZone;
    use crate::dep_groups::utils::bgp_zone_extractor;
    use crate::example_networks::repetitions::Repetition10;
    use crate::example_networks::{ChainGadgetLegacy, ExampleNetwork, Sigcomm, BipartiteGadget};
    use crate::netsim::types::Destination::*;
    use crate::netsim::Prefix;
    use crate::strategies::{Strategy, StrategyDAG, StrategyTRTA};
    use crate::topology_zoo::{Scenario, ZooTopology};
    use crate::Stopper;
    use std::collections::HashSet;
    use std::time::Instant;

    #[test]
    fn test_chain_gadget_partition() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let init_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let finl_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let patch = init_config.get_diff(&finl_config);
        let policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let strategy = StrategyZone::new(net, patch.modifiers, policy, None).unwrap();
        let zone = strategy.partition_zone();
        println!("{:?}", &zone);
    }

    #[test]
    fn test_chain_gadget_split_invariance() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let init_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let final_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let mut strategy =
            StrategyZone::new(net, init_config.get_diff(&final_config).modifiers, policy, None)
                .unwrap();
        strategy.work(Stopper::new()).expect("Panicked");
    }

    #[test]
    fn test_sigcomm_init_zones() {
        let net = Sigcomm::net(0);
        let start_config = Sigcomm::initial_config(&net, 0);
        let end_config = Sigcomm::final_config(&net, 0);
        let hard_policy = Sigcomm::get_policy(&net, 0);
        let mut strategy =
            StrategyZone::new(net, start_config.get_diff(&end_config).modifiers, hard_policy, None)
                .unwrap();
        strategy.init_zones().unwrap();
    }

    #[test]
    fn test_sigcomm_set_virtual_links() {
        let net = Sigcomm::net(0);
        let t1 = net.get_router_id("t1").unwrap();
        let t2 = net.get_router_id("t2").unwrap();
        let b1 = net.get_router_id("b1").unwrap();
        // let b2 = net.get_router_id("b2").unwrap();
        // let r1 = net.get_router_id("r1").unwrap();
        let r2 = net.get_router_id("r2").unwrap();
        let e1 = net.get_router_id("e1").unwrap();
        let e2 = net.get_router_id("e2").unwrap();
        let start_config = Sigcomm::initial_config(&net, 0);
        let end_config = Sigcomm::final_config(&net, 0);
        let hard_policy = Sigcomm::get_policy(&net, 0);
        let config_diff = start_config.get_diff(&end_config).modifiers;

        println!("{:?}", net.get_route(b1, Prefix(0)));

        let mut before = net.get_forwarding_state_new();
        let mut after = StrategyZone::get_after_states(&net, &config_diff).unwrap();
        let mut strategy = StrategyZone::new(net, config_diff.clone(), hard_policy, None).unwrap();
        strategy.init_zones().unwrap();
        for z in strategy.zones.values_mut() {
            // println!("Zone: {:?}, configs: {:?}", z.get_id(), z.get_configs());
            println!("Routers: {:?}", z.get_routers());
            println!("Configs: {:?}", z.get_configs());
            z.set_emulated_network(&mut before, &mut after).unwrap();
        }
        let mut fw_zone_1 =
            strategy.zones.get(&t1).unwrap().get_emulated_network().get_forwarding_state();
        let mut fw_zone_2 =
            strategy.zones.get(&t2).unwrap().get_emulated_network().get_forwarding_state();
        assert_eq!(fw_zone_1.get_route_new(t1, BGP(Prefix(0))), Ok(vec![t1, b1, e1]));
        assert_eq!(fw_zone_2.get_route_new(r2, BGP(Prefix(0))), Ok(vec![r2, e2]));
    }

    // It's interesting that StrategyTRTA tend to have huge runtime fluctuations over the same network
    // while StrategyZone's runtime is pretty stable

    // according to the runtime, generating the DAG consumes the most time

    #[test]
    fn test_sigcomm_net_synthesize() {
        let net = Sigcomm::net(0);
        let end_config = Sigcomm::final_config(&net, 0);
        let hard_policy = Sigcomm::get_policy(&net, 0);
        let dag =
            StrategyZone::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();
        println!("{:?}", dag);
    }

    #[test]
    fn test_abilene_net() {
        let net = BipartiteGadget::<Repetition10>::net(2);
        let end_config = BipartiteGadget::<Repetition10>::final_config(&net, 2);
        let hard_policy = BipartiteGadget::<Repetition10>::get_policy(&net, 2);
        let order = StrategyZone::synthesize(net, end_config, hard_policy, None, Stopper::new());
        println!("{:?}", order);
    }

    #[test]
    fn test_sigcomm_net_trta() {
        let net = Sigcomm::net(0);
        let end_config = Sigcomm::final_config(&net, 0);
        let hard_policy = Sigcomm::get_policy(&net, 0);
        let start_time = Instant::now();
        let _order =
            StrategyTRTA::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();
        let end_time = start_time.elapsed();

        println!("{:?}", end_time);
    }

    #[test]
    fn test_bgp_dfs() {
        let net = Sigcomm::net(0);
        // let external_router = net.get_external_routers()[0];
        // let device = net.get_device(external_router).unwrap_external();

        let b1 = net.get_router_id("b1").unwrap();
        let b2 = net.get_router_id("b2").unwrap();
        let t1 = net.get_router_id("t1").unwrap();
        let t2 = net.get_router_id("t2").unwrap();
        let r1 = net.get_router_id("r1").unwrap();
        let r2 = net.get_router_id("r2").unwrap();

        let set1 = bgp_zone_extractor(t1, &net);
        let set2 = bgp_zone_extractor(t2, &net);

        let res1: HashSet<_> = vec![b1, b2, r1, t1].into_iter().collect();
        let res2: HashSet<_> = vec![b2, r2, t2].into_iter().collect();

        assert_eq!(set1, Ok(res1));
        assert_eq!(set2, Ok(res2));
    }

    // For this part, the two algorithms has little difference in speed
    #[test]
    fn test_chain_gadget_synthesize() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let end_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let hard_policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let dag =
            StrategyZone::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();

        println!("{:?}", dag);
    }

    #[test]
    fn test_chain_gadget_trta() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let end_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let hard_policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);

        let start_time = Instant::now();
        let _order =
            StrategyTRTA::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();
        let end_time = start_time.elapsed();

        println!("{:?}", end_time);
    }

    #[test]
    // failed
    fn test_topo_zoo_uunet() {
        let file =
            String::from("/home/sheldonchen/Capstone/snowcap-plus/topology_zoo_gml/Uunet.gml");
        let (net, config, policy) = ZooTopology::new(&file, 42)
            .unwrap()
            .apply_scenario(Scenario::IntroduceSecondRouteReflector, false, 100, 1, 1.0)
            .unwrap();
        let order = StrategyZone::synthesize(net, config, policy, None, Stopper::new()).unwrap();
        println!("{:?}", order);
    }

    #[test]
    fn test_topo_zoo_york() {
        let file =
            String::from("/home/sheldonchen/Capstone/snowcap-plus/topology_zoo_gml/York.gml");
        let (net, config, policy) = ZooTopology::new(&file, 42)
            .unwrap()
            .apply_scenario(Scenario::IntroduceSecondRouteReflector, false, 100, 1, 1.0)
            .unwrap();
        let order = StrategyZone::synthesize(net, config, policy, None, Stopper::new()).unwrap();
        println!("{:?}", order);
    }

    #[test]
    fn test_topo_zoo_vtl() {
        let file =
            String::from("/home/sheldonchen/Capstone/snowcap-plus/topology_zoo_gml/VtlWavenet2011.gml");
        let (net, config, policy) = ZooTopology::new(&file, 42)
            .unwrap()
            .apply_scenario(Scenario::IntroduceSecondRouteReflector, false, 100, 1, 1.0)
            .unwrap();
        let order = StrategyZone::synthesize(net, config, policy, None, Stopper::new()).unwrap();
        println!("{:?}", order);
    }
    /*
        Topology zoo with many nodes
        Agis
        Amres
        York
        Xspedius
        VtlWavenet2011
        Uunet
        UsSignal
        UsCarrier
        Ntt
        Kdl (Kentucky data link)
    */
}
