use crate::hard_policies::{Condition, HardPolicy};
use crate::netsim::config::{ConfigExpr, ConfigModifier, ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::{BgpSessionType, ForwardingState, Network, NetworkDevice, Prefix, RouterId};
use crate::strategies::{Strategy, StrategyDAG};
use crate::{Error, Stopper};
use crate::dep_groups::utils::*;
use daggy::{Children, Dag, Parents};
use itertools::Itertools;
use log::error;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::time::Duration;

#[allow(unused_variables, unused_imports)]

pub type ZoneId = RouterId;

#[derive(Debug, Clone)]
pub struct Zone<'a> {
    pub id: ZoneId, // each zone is identified by the last router in the zone
    pub routers: HashSet<RouterId>,
    pub configs: Vec<&'a ConfigModifier>,
}

impl<'a> Zone<'a> {
    pub fn new(id: RouterId) -> Self {
        Self { id: id, routers: HashSet::<RouterId>::new(), configs: vec![] }
    }
    pub fn contains_router(&self, router: &RouterId) -> bool {
        self.routers.contains(router)
    }
    pub fn assign_routers(&mut self, routers: Vec<RouterId>) {
        routers.iter().for_each(|r| self.add_router(*r));
    }
    pub fn assign_configs(&mut self, configs: Vec<&'a ConfigModifier>) {
        configs.iter().for_each(|c| self.add_config(*c));
    }
    fn add_router(&mut self, router: RouterId) {
        self.routers.insert(router);
    }
    fn add_config(&mut self, config: &'a ConfigModifier) {
        self.configs.push(config);
    }
}

// type Zone = HashSet<RouterId>;

pub struct StrategyZone {
    net: Network,
    path_dependency: HashMap<RouterId, (Vec<RouterId>, Vec<RouterId>)>,
    // dag: Dag<ConfigModifier, u32, u32>,
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    stop_time: Option<Duration>,
}

impl StrategyDAG for StrategyZone {
    fn new(
        net: Network,
        modifiers: Vec<ConfigModifier>,
        hard_policy: HardPolicy,
        time_budget: Option<Duration>,
    ) -> Result<Box<Self>, crate::Error> {
        println!("{:?}", &hard_policy);
        Ok(Box::new(Self {
            net,
            path_dependency: HashMap::new(),
            // dag: daggy::
            modifiers,
            hard_policy,
            stop_time: time_budget,
        }))
    }

    fn work(&mut self, abort: Stopper) -> Result<Dag<ConfigModifier, u32, u32>, Error> {
        let mut zones = self.zone_partition();
        // println!("{:?}", zones);
        let (mut before_state, mut after_state) = self.get_before_after_states();
        let map = zone_into_map(&zones);
        // println!("{:?}", map);
        for p in &self.hard_policy.prop_vars {
            // how are these invariances organized?
            let (before_invariance, after_invariance) =
                self.split_invariances(p, &mut before_state, &mut after_state, &map)?;
        }
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

    fn bind_config_to_zone<'a>(&'a self, mut zones: HashMap<RouterId, Zone<'a>>) -> HashMap<RouterId, Zone>{
        // let net = &self.net;
        let configs = &self.modifiers;
        // let mut zone_configs = Vec::<ZoneConfig>::with_capacity(zones.len());
        for (_, z) in &mut zones {
            // println!("{:?}", z);
            // let mut zone_configs = Vec::<&ConfigModifier>::new();
            for config in configs {
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
                                    z.add_config(config);
                                }
                                (true, false) => {
                                    // if the source router is in the zone and is a client, or is an eBGP
                                    if *session_type == BgpSessionType::IBgpClient
                                        || *session_type == BgpSessionType::EBgp
                                    {
                                        z.add_config(config);
                                    } else {
                                        // or the target router is a route reflector/boundary router
                                        if self.is_client_or_boundary(target)
                                            && !self.is_self_client(source, target)
                                        {
                                            z.add_config(config);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else if let ConfigExpr::BgpRouteMap { router, direction: _, map: _ } = c {
                            if z.contains_router(router) {
                                z.add_config(config);
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
                                (true, true) => z.add_config(config),
                                (true, false)
                                    if *session1 == BgpSessionType::IBgpClient
                                        || *session2 == BgpSessionType::IBgpClient =>
                                {
                                    z.add_config(config);
                                }
                                _ => {}
                            }
                        }
                        (
                            ConfigExpr::BgpRouteMap { router, direction: _, map: _ },
                            ConfigExpr::BgpRouteMap { router: _, direction: _, map: _ },
                        ) => {
                            if z.contains_router(router) {
                                z.add_config(config);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        zones
    }

    fn split_invariances(
        &self,
        hard_policy: &Condition,
        before_state: &mut ForwardingState,
        after_state: &mut ForwardingState,
        map: &HashMap<RouterId, Vec<ZoneId>>,
    ) -> Result<(Vec<HardPolicy>, Vec<HardPolicy>), Error> {
        match hard_policy {
            Condition::Reachable(r, p, _) | Condition::NotReachable(r, p) => {
                let (before_path, after_path) =
                    extract_paths_for_router(*r, *p, before_state, after_state);
                // println!("{:?}\n{:?}\n", &before_path, &after_path);
                // segment routers on new route and old route into different zones
                // println!("{:?}", &before_path);
                let before_zones = segment_path(map, &before_path);
                let after_zones = segment_path(map, &after_path);
                // create a new set of invariances according to these zones
                println!("{:?}", before_zones);
            }
            _ => {}
        }
        Ok((vec![], vec![]))
    }

    fn get_before_after_states(&self) -> (ForwardingState, ForwardingState) {
        let mut after_net = self.get_after_net();
        (self.net.get_forwarding_state(), after_net.get_forwarding_state())
    }

    fn get_after_net(&self) -> Network {
        let mut after_net = self.net.clone();
        for c in self.modifiers.iter() {
            // this may not work for topologies that take into account time (e.g. Difficult gadget)
            after_net.apply_modifier(c);
        }
        after_net
    }

    fn is_client_or_boundary(&self, rid: &RouterId) -> bool {
        let router = self.net.get_device(*rid).unwrap_internal();
        let result = router
            .bgp_sessions
            .iter()
            .map(|(id, session)| {
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

// #[derive(Debug, Clone)]
/// A struct storing the configurations relevant to a zone
// pub struct ZoneConfig<'a> {
//     /// Identifier of the struct, defined by the zone's ending router's id
//     pub zone_id: RouterId,
//     /// Vector storing configurations relevant to the current zone
//     pub relevant_configs: Vec<&'a ConfigModifier>,
// }

// impl<'a> ZoneConfig<'a> {
//     pub fn new(id: RouterId) -> ZoneConfig<'a> {
//         Self {
//             zone_id: id,
//             relevant_configs: vec![]
//         }
//     }

//     pub fn add(&mut self, config: &'a ConfigModifier) {
//         self.relevant_configs.push(config);
//     }
// }

/// For each non-route-reflector internal device, find the zone it belongs to
/// returned in a HashMap, key is the internal router's id, value is the associated zone
// In the future can be modified to any router that maintains reachability conditions
// Issue: do we need a better representation for the zone? (maybe use a dag?)

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::vec;
    use crate::hard_policies::HardPolicy;
    use crate::netsim::RouterId;
    use crate::dep_groups::strategy_zone::{ZoneId, Zone, StrategyZone};
    use crate::example_networks::repetitions::{Repetition10, Repetition5};
    use crate::example_networks::{ChainGadgetLegacy, ExampleNetwork, FirewallNet, AbileneNetwork, BipartiteGadget, ChainGadget};
    use crate::netsim::Network;
    use crate::strategies::StrategyDAG;
    use crate::Stopper;
    use daggy::walker::Chain;
    use petgraph::prelude::NodeIndex;

    #[test]
    fn test_chain_gadget_partition() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let init_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let finl_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let patch = init_config.get_diff(&finl_config);
        let policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let strategy = StrategyZone::new(net, patch.modifiers, policy, None).unwrap();
        let zone = strategy.zone_partition();
    }

    #[test]
    fn test_chain_gadget_split_invariance() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let init_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let final_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let mut strategy = StrategyZone::new(
            net,
            init_config.get_diff(&final_config).modifiers,
            policy,
            None
        ).unwrap();
        strategy.work(Stopper::new());
    }

    #[test]
    fn test_firewall_net_partition() {
        let net = FirewallNet::net(0);
        // let map = strategy_zone::zone_partition(&net);
        // // println!("{:?}", map);
        // strategy_zone::zone_pretty_print(&net, &map);
    }

    #[test]
    fn test_abilene_net_partition() {
        let net = AbileneNetwork::net(0);
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

        let path = vec![t1, t0, b0];
        let path2 = vec![t1, b1];
        let path3 = vec![r0, r1, b0];
        let path4 = vec![r1, r0, b1];
        let path5 = vec![r1, b0];

        // assert_eq!(StrategyZone::segment_path(&map, &path), vec![vec![t1], vec![t0, b0]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path2), vec![vec![t1, b1]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path3), vec![vec![r0], vec![r1], vec![b0]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path4), vec![vec![r1], vec![r0, b1]]);
        // assert_eq!(StrategyZone::segment_path(&map, &path5), vec![vec![r1], vec![b0]]);
    }
}
