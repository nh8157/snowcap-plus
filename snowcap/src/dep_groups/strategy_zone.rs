use crate::netsim::config::{ConfigModifier, ConfigExpr};
use crate::netsim::{Network, RouterId, NetworkDevice, BgpSessionType};
use crate::netsim::router::Router;
use std::collections::{HashSet, HashMap};

type Zone = HashSet<RouterId>;

/// A struct storing the configurations relevant to a zone
pub struct ZoneConfig<'a> {
    /// Identifier of the struct, defined by the zone's ending router's id
    zone_id: RouterId,
    /// Vector storing configurations relevant to the current zone
    relevant_configs: Vec<&'a ConfigModifier>,
}

impl<'a> ZoneConfig<'a> {
    pub fn new(id: RouterId) -> ZoneConfig<'a> {
        Self {
            zone_id: id,
            relevant_configs: vec![]
        }
    }

    pub fn add(&mut self, config: &'a ConfigModifier) {
        self.relevant_configs.push(config);
    }
}

/// For each non-route-reflector internal device, find the zone it belongs to
/// returned in a HashMap, key is the internal router's id, value is the associated zone
// In the future can be modified to any router that maintains reachability conditions
// Issue: do we need a better representation for the zone? (maybe use a dag?)
fn zone_partition(net: &Network) -> HashMap<RouterId, Zone> {
    let mut zones = HashMap::<RouterId, Zone>::new();
    let router_ids = net.get_routers();
    'outer: for id in &router_ids {
        let router = net.get_device(*id).unwrap_internal();

        // Determine if the current router qualifies as the endpoint of a zone
        for (peer_id, session_type) in &router.bgp_sessions {
            match *session_type {
                // Only the router that is not a route reflector nor a boundary router can be added
                BgpSessionType::EBgp => continue 'outer,
                BgpSessionType::IBgpPeer => {
                    let peer_router = net.get_device(*peer_id).unwrap_internal();
                    // if my peer router's session type is iBGP client, then I am a route reflector
                    if BgpSessionType::IBgpClient == peer_router.bgp_sessions[id] {
                        continue 'outer;
                    }
                }
                _ => {}
            }
        }

        // Runs a BFS to identify parents, store the next level in a vector (queue)
        let mut level = vec![*id];
        let mut zone = Zone::new();
        while level.len() != 0 {
            let current_id = level.remove(0);
            if !zone.contains(&current_id) {
                let current_router = net.get_device(current_id).unwrap_internal();
                zone.insert(current_id);
                
                // Determine if any neighboring routers can be included in the zone
                for (peer_id, session) in &current_router.bgp_sessions {
                    match *session {
                        // Current router is the client of its route reflector peer
                        BgpSessionType::IBgpClient => level.push(*peer_id),
                        // Current router is a peer of the peer
                        // Peer is a valid zone router iff it is a boundary router or a route reflector
                        BgpSessionType::IBgpPeer => {
                            let peer_router = net.get_device(*peer_id).unwrap_internal();
                            // Search through the bgp sessions of the peer and find if the peer router is a client
                            // of another router that is not the current router, or a boundary router
                            let is_other_client = peer_router.bgp_sessions
                                .iter()
                                .map(|(i, s)| 
                                    (*i != current_id) && ((*s == BgpSessionType::IBgpClient) || (*s == BgpSessionType::EBgp)))
                                .fold(false, |acc, x| (acc | x));
                            // Determine if the peer router is a client of the current router
                            let is_current_client = peer_router.bgp_sessions[&current_id] == BgpSessionType::IBgpClient;
                            if !is_current_client && is_other_client {
                                level.push(*peer_id);
                            }
                        }
                        _ => {}
                    }
                }
                // } 
            }
        }
        // push the current zone into the final collection of zones
        zones.insert(*id, zone);
    }
    // for each internal router, reversely find its parents and grandparents
    zones
}

fn bind_config_to_zone(zones: &HashMap<RouterId, Zone>, configs: &Vec<ConfigModifier>) {
    let mut zone_configs = Vec::<ZoneConfig>::with_capacity(zones.len());
    for (zid, z) in zones {
        let mut zone_config = ZoneConfig::new(*zid);
        for config in configs {
            match config {
                ConfigModifier::Insert(c) | ConfigModifier::Remove(c) => {
                    // Only implement zone partitioning for BGP Session configurations
                    if let ConfigExpr::BgpSession { 
                        source: source, target: target, session_type: session 
                    } = c {
                        // Test if both routers are in zone
                        let routers_in_zone = (z.contains(source), z.contains(target));
                        match routers_in_zone {
                            // both routers are in the zone
                            (true, true) => zone_config.add(config),
                            // only the source router is in the zone and is a client, or is an eBGP
                            (true, false) if *session == BgpSessionType::IBgpClient || *session == BgpSessionType::EBgp => {
                                zone_config.add(config);
                            },
                            _ => {}
                        }
                    }
                }
                ConfigModifier::Update {from: c1, to: c2 } => {
                    match (c1, c2) {
                        (
                            ConfigExpr::BgpSession { source: s1, target: t1, session_type: session1 },
                            ConfigExpr::BgpSession { source: s2, target: t2, session_type: session2 }
                        ) => {
                            let routers_in_zone = (z.contains(s1), z.contains(t1));
                            match routers_in_zone {
                                (true, true) => zone_config.add(config),
                                (true, false) if *session1 == BgpSessionType::IBgpClient || *session2 == BgpSessionType::IBgpClient => {
                                    zone_config.add(config);
                                },
                                _ => {}
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
    }
}
fn zone_pretty_print(net: &Network, map: &HashMap<RouterId, HashSet<RouterId>>) {
    for (id, set) in map {
        let router_name = net.get_router_name(*id).unwrap();
        println!("Zone of router {}", router_name);
        set.iter().for_each(|n| {
            let parent_router_name = net.get_router_name(*n).unwrap();
            println!("\t{}", parent_router_name);
        })
    }
}

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_zone;
    use crate::example_networks::repetitions::{Repetition10, Repetition5};
    use crate::example_networks::{ChainGadgetLegacy, ExampleNetwork, self};
    use crate::netsim::Network;

    #[test]
    fn test_chain_gadget_partition() {
        let net = example_networks::ChainGadgetLegacy::<Repetition10>::net(0);
        let map = strategy_zone::zone_partition(&net);
        // println!("{:?}", map);
        strategy_zone::zone_pretty_print(&net, &map);
    }
    
    #[test]
    fn test_bipartite_gadget_partition() {
        let net = example_networks::BipartiteGadget::<Repetition5>::net(2);
        let map = strategy_zone::zone_partition(&net);
        // println!("{:?}", map);
        strategy_zone::zone_pretty_print(&net, &map);
    }

    #[test]
    fn test_firewall_net_partition() {
        let net = example_networks::FirewallNet::net(0);
        let map = strategy_zone::zone_partition(&net);
        // println!("{:?}", map);
        strategy_zone::zone_pretty_print(&net, &map);
    }
}