use crate::hard_policies::{Condition, HardPolicy};
use crate::netsim::config::{ConfigExpr, ConfigModifier};
// use crate::netsim::router::Router;
use crate::dep_groups::utils::*;
use crate::netsim::{BgpSessionType, ForwardingState, Network, Prefix, RouterId};
use crate::strategies::StrategyDAG;
use crate::{Error, Stopper};
use daggy::Dag;
use log::*;
use petgraph::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[allow(unused_variables, unused_imports)]

pub type ZoneId = RouterId;

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: ZoneId, // each zone is identified by the last router in the zone
    pub routers: HashSet<RouterId>,
    // index of the configurations
    // could be updated to index of the configurations instead
    pub configs: Vec<usize>,
    // a field holding the associated hard policies are also necessary
    hard_policy: HashSet<Condition>,
}

impl Zone {
    pub fn new(id: RouterId) -> Self {
        Self {
            id: id,
            routers: HashSet::<RouterId>::new(),
            configs: Vec::<usize>::new(),
            hard_policy: HashSet::<Condition>::new(),
        }
    }
    pub fn contains_router(&self, router: &RouterId) -> bool {
        self.routers.contains(router)
    }
    pub fn assign_routers(&mut self, routers: Vec<RouterId>) {
        routers.iter().for_each(|r| self.add_router(*r));
    }
    // pub fn assign_configs(&mut self, configs: Vec<usize>) {
    //     configs.iter().for_each(|c| self.add_config(*c));
    // }
    fn add_router(&mut self, router: RouterId) {
        self.routers.insert(router);
    }
    fn add_config(&mut self, config: usize) {
        self.configs.push(config);
    }
    fn add_hard_policy(&mut self, condition: Condition) {
        if !self.hard_policy.contains(&condition) {
            self.hard_policy.insert(condition);
        }
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
        println!("Begin work");
        self.zones = self.zone_partition();
        let (mut before_state, mut after_state) = self.get_before_after_states()?;
        let router_to_zone = zone_into_map(&self.zones);
        println!("{:?}", router_to_zone);

        // First verify if the hard policy is global reachability
        // The current algorithm cannot handle other LTL modals
        if !self.hard_policy.is_global_reachability() {
            return Err(Error::NotImplemented);
        }
        let condition = self.hard_policy.prop_vars.clone();
        println!("Conditions retrieved");
        // partition each invariance according to their zones
        // might need to check the temporal modal as well?
        for c in &condition {
            match c {
                // only support reachable/not reachable condtions
                Condition::Reachable(r, p, _) => {
                    let (before_path, after_path) =
                        extract_paths_for_router(*r, *p, &mut before_state, &mut after_state);

                    // segment routers on new route and old route into different zones
                    let before_zones = segment_path(&router_to_zone, &before_path)?;
                    let after_zones = segment_path(&router_to_zone, &after_path)?;
                    // create propositional variables using these zones
                    self.segment_invariance_to_zones(p, &before_zones, &router_to_zone)?;
                    self.segment_invariance_to_zones(p, &after_zones, &router_to_zone)?;
                }
                // Might need adaptation for condition NotReachable
                _ => {
                    error!("Can only handle condition reachable");
                    return Err(Error::NotImplemented);
                }
            }
        }
        println!("{:?}", self.zones);
        Ok(Dag::<ConfigModifier, u32, u32>::new())
    }
}

impl StrategyZone {
    fn zone_partition(&self) -> HashMap<RouterId, Zone> {
        let net = &self.net;
        let mut zones = HashMap::<RouterId, Zone>::new();
        let router_ids = net.get_routers();
        'outer: for id in &router_ids {
            let router = net.get_device(*id).unwrap_internal();

            // Determine if the current router can no longer propagate the advertisement
            for (peer_id, session_type) in &router.bgp_sessions {
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

            // Runs a BFS to identify parents, store the next level in a vector (queue)
            let mut level = vec![*id];
            let mut zone = Zone::new(*id);
            let mut router_set = HashSet::<RouterId>::new();
            while level.len() != 0 {
                let current_id = level.remove(0);
                if !router_set.contains(&current_id) {
                    let current_router = net.get_device(current_id).unwrap_internal();
                    router_set.insert(current_id);
                    // Determine if any neighboring routers can be included in the zone
                    for (peer_id, session) in &current_router.bgp_sessions {
                        match *session {
                            // Current router is the client of its route reflector peer
                            BgpSessionType::IBgpClient => level.push(*peer_id),
                            // Current router is a peer of the peer
                            // Peer is a valid zone router iff it is a boundary router or a route reflector
                            BgpSessionType::IBgpPeer => {
                                // Determine if the peer router is a client of the current router
                                if self.is_client_or_boundary(peer_id)
                                    && !self.is_self_client(&current_id, peer_id)
                                {
                                    level.push(*peer_id);
                                }
                            }
                            _ => {}
                        }
                    }
                    // }
                }
            }
            zone.assign_routers(router_set.into_iter().collect());
            // push the current zone into the final collection of zones
            zones.insert(*id, zone);
        }
        self.bind_config_to_zone(zones)
        // for each internal router, reversely find its parents and grandparents
    }

    fn bind_config_to_zone(&self, mut zones: HashMap<RouterId, Zone>) -> HashMap<RouterId, Zone> {
        // let net = &self.net;
        // let mut zone_configs = Vec::<ZoneConfig>::with_capacity(zones.len());
        for (_, z) in &mut zones {
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
        zones
    }

    fn segment_invariance_to_zones(
        &mut self,
        p: &Prefix,
        segmented_paths: &Vec<Vec<NodeIndex>>,
        router_to_zone: &HashMap<RouterId, Vec<ZoneId>>,
    ) -> Result<(), Error> {
        for x in segmented_paths {
            let router = x.first().unwrap();
            if let Some(current_zones) = router_to_zone.get(router) {
                // what if the router belongs to multiple zones? do we need to add them to zone one by one?
                current_zones.iter().for_each(|x| {
                    if self.zones.contains_key(x) {
                        let ptr = self.zones.get_mut(x).unwrap();
                        ptr.add_hard_policy(Condition::Reachable(*router, *p, None));
                    }
                });
            }
            // This part is currently unusable
            // because some routers may not belong to any zone before reconfiguration
            /*
               else {
                   error!("Router does not exist in zone");
                   return Err(Error::ZoneSegmentationFailed);
               }
            */
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
// Issue: do we need a better representation for the zone? (maybe use a dag?)

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_zone::{StrategyZone, ZoneId};
    use crate::example_networks::repetitions::Repetition10;
    use crate::example_networks::{ChainGadgetLegacy, ExampleNetwork};
    // use crate::hard_policies::HardPolicy;
    // use crate::netsim::Network;
    use crate::netsim::RouterId;
    use crate::strategies::StrategyDAG;
    use crate::Stopper;
    use petgraph::prelude::NodeIndex;
    use std::collections::HashMap;
    use std::vec;

    #[test]
    fn test_chain_gadget_partition() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let init_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let finl_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let patch = init_config.get_diff(&finl_config);
        let policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let strategy = StrategyZone::new(net, patch.modifiers, policy, None).unwrap();
        let zone = strategy.zone_partition();
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
    fn test_firewall_net_partition() {
        // let net = FirewallNet::net(0);
        // let map = strategy_zone::zone_partition(&net);
        // // println!("{:?}", map);
        // strategy_zone::zone_pretty_print(&net, &map);
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
