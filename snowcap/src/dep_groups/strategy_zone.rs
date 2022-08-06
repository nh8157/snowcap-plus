use crate::hard_policies::{Condition, HardPolicy};
use crate::netsim::config::{ConfigExpr, ConfigModifier};
// use crate::netsim::router::Router;
use crate::dep_groups::utils::*;
use crate::netsim::{BgpSessionType, ForwardingState, Network, NetworkError, Prefix, RouterId};
use crate::strategies::StrategyDAG;
use crate::{Error, Stopper};
use daggy::Dag;
use log::*;
use petgraph::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// #[allow(unused_variables, unused_imports)]

pub type ZoneId = RouterId;

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: ZoneId, // each zone is identified by the last router in the zone
    pub routers: HashSet<RouterId>,
    // index of the configurations
    pub configs: Vec<usize>,
    num_of_routers: usize,
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
            num_of_routers: 0,
            hard_policy: HashSet::new(),
            virtual_boundary_routers: HashSet::new(),
            emulated_network: net.to_owned(),
        }
    }
    fn contains_router(&self, router: &RouterId) -> bool {
        self.routers.contains(router)
    }
    fn assign_routers(&mut self, routers: Vec<RouterId>) {
        routers.iter().for_each(|r| self.add_router(*r));
        self.num_of_routers = routers.len();
    }
    fn add_router(&mut self, router: RouterId) {
        self.routers.insert(router);
    }
    fn set_emulated_network(
        &mut self,
        before_state: &mut ForwardingState,
        after_state: &mut ForwardingState,
    ) -> Result<(), NetworkError> {
        // Step 1: Identify and initialize virtual boundary routers
        self.init_virtual_boundary_routers()?;

        // Step 2: Iterate through all virtual routers in the set and check for every prefix whether the next hop is outside of the zone.
        // If either of the next hop is outside of the zone, we add a virtual link to the corresponding router,
        // else we don't add any link
        for virtual_boundary_router in self.get_virtual_boundary_routers() {
            let mut virtual_links = HashMap::new();
            for prefix in self.emulated_network.get_known_prefixes() {
                let before_route =
                    before_state.get_route(virtual_boundary_router, *prefix).unwrap();
                if !self.get_routers().contains(&before_route[1]) {
                    let ptr = virtual_links.entry(before_route[1]).or_insert(HashSet::new());
                    ptr.insert(*before_route.last().unwrap());
                }
                let after_route = after_state.get_route(virtual_boundary_router, *prefix).unwrap();
                if !self.get_routers().contains(&after_route[1]) {
                    let ptr = virtual_links.entry(after_route[1]).or_insert(HashSet::new());
                    ptr.insert(*after_route.last().unwrap());
                }
            }
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
    fn get_routers(&self) -> HashSet<RouterId> {
        self.routers.clone()
    }
    fn get_virtual_boundary_routers(&self) -> HashSet<RouterId> {
        self.virtual_boundary_routers.clone().into_iter().collect()
    }
    fn get_emulated_network(&self) -> &Network {
        &self.emulated_network
    }
}

pub struct StrategyZone {
    net: Network,
    zones: HashMap<ZoneId, Zone>,
    // path_dependency: HashMap<RouterId, (Vec<RouterId>, Vec<RouterId>)>,
    // dag: Dag<ConfigModifier, u32, u32>,
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    _stop_time: Option<Duration>,
}

impl StrategyDAG for StrategyZone {
    fn new(
        net: Network,
        modifiers: Vec<ConfigModifier>,
        hard_policy: HardPolicy,
        time_budget: Option<Duration>,
    ) -> Result<Box<Self>, crate::Error> {
        Ok(Box::new(Self {
            net,
            zones: HashMap::<ZoneId, Zone>::new(),
            // path_dependency: HashMap::new(),
            // dag: daggy::
            modifiers,
            hard_policy,
            _stop_time: time_budget,
        }))
    }

    fn work(&mut self, _abort: Stopper) -> Result<Dag<ConfigModifier, u32, u32>, Error> {
        // Need to clean up this code
        let (mut before_state, mut after_state) = self.get_before_after_states()?;
        // let mut zones = self.partition_zone();
        // self.bind_config_to_zone(&mut zones);
        // self.zones = zones;
        self.init_zones()?;

        // Iterate over each zone identified
        for zone in self.zones.values_mut() {
            zone.set_emulated_network(&mut before_state, &mut after_state)?;
            // Step 1: For each zone, iterate through all routers, identify virtual boundary routers
            // zone.init_virtual_boundary_routers()?;

            // // Step 2: For each boundary device, get their next hop to reach prefixes in the network before and after reconfiguration
            // // !!! This algorithm may need further improvement !!!
            // for virtual_boundary_router in zone.get_virtual_boundary_routers() {
            //     let mut virtual_links = HashMap::<RouterId, HashSet<RouterId>>::new();
            //     for prefix in self.net.get_known_prefixes() {
            //         let route_before = before_state.get_route(virtual_boundary_router, *prefix)?;
            //         if !zone.get_routers().contains(&route_before[1]) {
            //             // insert this virtual link into the map
            //             let ptr = virtual_links.entry(route_before[1]).or_insert(HashSet::<RouterId>::new());
            //             ptr.insert(*route_before.last().unwrap());
            //         }
            //         let route_after = after_state.get_route(virtual_boundary_router, *prefix)?;
            //         if !zone.get_routers().contains(&route_after[1]) {
            //             let ptr = virtual_links.entry(route_after[1]).or_insert(HashSet::<RouterId>::new());
            //             ptr.insert(*route_after.last().unwrap());
            //         }
            //     }
            //     println!("{:?}", virtual_links);
            //     // Set this virtual zone onto the corresponding boundary router
            //     zone.emulated_network.set_virtual_links_for_router(virtual_boundary_router, virtual_links)?;
            // }
        }

        let router_to_zone = zone_into_map(&self.zones);

        // First verify if the hard policy is global reachability
        // The current algorithm cannot handle other LTL modals
        if !self.hard_policy.is_global_reachability() {
            return Err(Error::NotImplemented);
        }
        let condition = self.hard_policy.prop_vars.clone();
        // partition each invariance according to their zones
        // might need to check the temporal modal as well?
        for c in &condition {
            match c {
                // only support reachable/not reachable condtions
                Condition::Reachable(r, p, _) => {
                    let (before_path, after_path) =
                        extract_paths_for_router(*r, *p, &mut before_state, &mut after_state);
                    // also need to retrieve the AS info of this router
                    // the last hop must end at an external router
                    // segment routers on new route and old route into different zones
                    let before_zones = segment_path(&router_to_zone, &before_path)?;
                    let after_zones = segment_path(&router_to_zone, &after_path)?;
                    // create propositional variables and virtual boundaries using these zones
                    // build path-wise dependencies
                    self.split_invariance(p, &before_zones, &router_to_zone)?;
                    self.split_invariance(p, &after_zones, &router_to_zone)?;
                }
                // Might need adaptation for condition NotReachable
                _ => {
                    error!("Can only handle condition reachable");
                    return Err(Error::NotImplemented);
                }
            }
        }
        // println!("{:?}", self.zones);
        Ok(Dag::<ConfigModifier, u32, u32>::new())
    }
}

impl StrategyZone {
    fn init_zones(&mut self) -> Result<(), Error> {
        let mut zones = self.partition_zone();
        self.bind_config_to_zone(&mut zones);
        self.zones = zones;
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
            zone.assign_routers(router_set.into_iter().collect());
            // push the current zone into the final collection of zones
            zones.insert(*id, zone);
        }
        // self.bind_config_to_zone(&mut zones);
        // then we construct a virtual boundary by adding symbolic links between virtual boundary routers and external routers
        zones
    }

    fn bind_config_to_zone(&self, zones: &mut HashMap<RouterId, Zone>) {
        // let net = &self.net;
        // let mut zone_configs = Vec::<ZoneConfig>::with_capacity(zones.len());
        for (_, z) in zones {
            // println!("{:?}", z);
            // let mut zone_configs = Vec::<&ConfigModifier>::new();
            for (idx, config) in self.modifiers.iter().enumerate() {
                match config {
                    ConfigModifier::Insert(c) | ConfigModifier::Remove(c) => {
                        // Only implement zone partitioning for BGP Session configurations
                        if let ConfigExpr::BgpSession { source, target, session_type } = c {
                            // Test if both routers are in zone
                            let routers_in_zone =
                                (z.contains_router(source), z.contains_router(target));
                            // println!("{:?}", config);
                            match routers_in_zone {
                                // both routers are in the zone
                                (true, true) => {
                                    z.add_config(idx);
                                }
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

    fn split_invariance(
        &mut self,
        p: &Prefix,
        segmented_paths: &Vec<Vec<NodeIndex>>,
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
                    for z in intersect {
                        if self.zones.contains_key(*z) {
                            let zone_ptr = self.zones.get_mut(*z).unwrap();
                            zone_ptr.add_hard_policy(Condition::Reachable(*router, *p, None));
                        }
                    }
                }
                _ => continue,
            }
        }
        Ok(())
    }

    fn get_before_after_states(&self) -> Result<(ForwardingState, ForwardingState), Error> {
        let after_net = self.get_after_net()?;
        Ok((self.net.get_forwarding_state(), after_net.get_forwarding_state()))
    }

    fn get_after_net(&self) -> Result<Network, Error> {
        let mut after_net = self.net.clone();
        for c in self.modifiers.iter() {
            // this may not work for topologies that take into account time (e.g. Difficult gadget)
            after_net.apply_modifier(c)?;
        }
        Ok(after_net)
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

    // fn zone_pretty_print(net: &Network, map: &HashMap<RouterId, HashSet<RouterId>>) {
    //     for (id, set) in map {
    //         let router_name = net.get_router_name(*id).unwrap();
    //         println!("Zone of router {}", router_name);
    //         set.iter().for_each(|n| {
    //             let parent_router_name = net.get_router_name(*n).unwrap();
    //             println!("\t{}", parent_router_name);
    //         })
    //     }
    // }

    // fn zone_config_pretty_print(net: &Network, zone_configs: &Vec<ZoneConfig>) {
    //     for z in zone_configs {
    //         let name = net.get_router_name(z.zone_id).unwrap();
    //         println!("{}", name);
    //         for c in &z.relevant_configs {
    //             println!("\t{:?}", *c);
    //         }
    //     }
    // }
}

/// For each non-route-reflector internal device, find the zone it belongs to
/// returned in a HashMap, key is the internal router's id, value is the associated zone
// In the future can be modified to any router that maintains reachability conditions

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_zone::{StrategyZone, ZoneId};
    use crate::dep_groups::utils::bgp_zone_extractor;
    use crate::example_networks::repetitions::Repetition10;
    use crate::example_networks::{ChainGadgetLegacy, ExampleNetwork, Sigcomm};
    // use crate::hard_policies::HardPolicy;
    // use crate::netsim::Network;
    use crate::netsim::{RouterId, Prefix};
    use crate::strategies::StrategyDAG;
    use crate::Stopper;
    use petgraph::prelude::NodeIndex;
    use std::collections::{HashMap, HashSet};
    use std::vec;

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
        let b2 = net.get_router_id("b2").unwrap();
        let r1 = net.get_router_id("r1").unwrap();
        let r2 = net.get_router_id("r2").unwrap();
        let e1 = net.get_router_id("e1").unwrap();
        let e2 = net.get_router_id("e2").unwrap();
        let start_config = Sigcomm::initial_config(&net, 0);
        let end_config = Sigcomm::final_config(&net, 0);
        let hard_policy = Sigcomm::get_policy(&net, 0);
        let mut strategy =
            StrategyZone::new(net, start_config.get_diff(&end_config).modifiers, hard_policy, None)
                .unwrap();
        let (mut before, mut after) = strategy.get_before_after_states().unwrap();
        strategy.init_zones().unwrap();
        for z in strategy.zones.values_mut() {
            z.set_emulated_network(&mut before, &mut after).unwrap();
        }
        assert_eq!(strategy.zones.get(&t1).unwrap().get_emulated_network().get_route(r1, Prefix(0)).unwrap(), vec![r1, b2, e2]);
        assert_eq!(strategy.zones.get(&t1).unwrap().get_emulated_network().get_route(t1, Prefix(0)).unwrap(), vec![t1, b1, e1]);
        assert_eq!(strategy.zones.get(&t2).unwrap().get_emulated_network().get_route(r2, Prefix(0)).unwrap(), vec![r2, e2]);
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

    #[test]
    fn test_abilene_net_partition() {
        // let net = AbileneNetwork::net(0);
        // let map = strategy_zone::zone_partition(&net);
        // let init_config = example_networks::AbileneNetwork::initial_config(&net, 0);
        // let final_config = example_networks::AbileneNetwork::final_config(&net, 0);
        // let diff = init_config.get_diff(&final_config);
        // println!("{:?}", diff);
        // strategy_zone::zone_pretty_print(&net, &map);
    }

    #[test]
    fn test_firewall_net_config_binding() {
        // let net = example_networks::FirewallNet::net(0);
        // let init_config = example_networks::FirewallNet::initial_config(&net, 0);
        // let final_config = example_networks::FirewallNet::final_config(&net, 0);
        // let patch = init_config.get_diff(&final_config);
        // println!("{:?}", patch);
        // let zones = strategy_zone::zone_partition(&net);
        // strategy_zone::bind_config_to_zone(&net, &mut zones, &patch);
        // println!("{:?}", zone_configs);
    }

    #[test]
    fn test_bipartite_net_config_binding() {
        // let net = example_networks::BipartiteGadget::<Repetition10>::net(2);
        // let init_config =
        //     example_networks::BipartiteGadget::<Repetition10>::initial_config(&net, 2);
        // let final_config = example_networks::BipartiteGadget::<Repetition10>::final_config(&net, 2);
        // let patch = init_config.get_diff(&final_config);
        // println!("{:?}", patch);
        // let zones = strategy_zone::zone_partition(&net);
        // println!("Reporting configuration bindings");
        // let zone_configs = strategy_zone::bind_config_to_zone(&net, &zones, &patch);
        // // println!("{:?}", zone_configs);
        // strategy_zone::zone_config_pretty_print(&net, &zone_configs);
    }

    #[test]
    fn test_chain_gadget_config_binding() {
        // let net = example_networks::ChainGadgetLegacy::<Repetition10>::net(0);
        // let map = strategy_zone::zone_partition(&net);
        // println!("{:?}", map);
        // // strategy_zone::zone_pretty_print(&net, &map);
        // let init_config =
        //     example_networks::ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        // let final_config =
        //     example_networks::ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        // let patch = init_config.get_diff(&final_config);

        // println!("{:?}", patch);
        // let zones = strategy_zone::zone_partition(&net);
        // println!("Reporting configuration bindings");
        // let zone_configs = strategy_zone::bind_config_to_zone(&net, &zones, &patch);
        // // println!("{:?}", zone_configs);
        // strategy_zone::zone_config_pretty_print(&net, &zone_configs);
    }

    #[test]
    fn test_segment_path() {
        let mut map = HashMap::<RouterId, Vec<ZoneId>>::new();
        let b0 = NodeIndex::new(0);
        let b1 = NodeIndex::new(1);
        let r0 = NodeIndex::new(2);
        let r1 = NodeIndex::new(3);
        let t0 = NodeIndex::new(4);
        let t1 = NodeIndex::new(5);
        map.insert(b0, vec![t0]);
        map.insert(b1, vec![t0, t1]);
        map.insert(r0, vec![t0]);
        map.insert(r1, vec![t1]);
        map.insert(t0, vec![t0]);
        map.insert(t1, vec![t1]);

        // let path = vec![t1, t0, b0];
        // let path2 = vec![t1, b1];
        // let path3 = vec![r0, r1, b0];
        // let path4 = vec![r1, r0, b1];
        // let path5 = vec![r1, b0];

        // assert_eq!(StrategyZone::segment_path(&map, &path), vec![vec![t1], vec![t0, b0]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path2), vec![vec![t1, b1]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path3), vec![vec![r0], vec![r1], vec![b0]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path4), vec![vec![r1], vec![r0, b1]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path5), vec![vec![r1], vec![b0]]);
    }
}
