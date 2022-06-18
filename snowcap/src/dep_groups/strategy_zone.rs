use crate::netsim::{Network, RouterId, NetworkDevice, BgpSessionType};
use crate::netsim::router::Router;
use std::collections::{HashSet, HashMap};
use std::thread::current;

/// For each non-route-reflector internal device, find the zone it belongs to
/// returned in a HashMap, key is the internal router's id, value is the associated zone
fn zone_partition(net: &Network) -> HashMap<RouterId, HashSet<RouterId>> {
    let mut zones = HashMap::<RouterId, HashSet<RouterId>>::new();
    let router_ids = net.get_routers();
    'outer: for id in &router_ids {
        // only evaluates internal routers
        if let NetworkDevice::InternalRouter(router) = net.get_device(*id) {
            // valid only when all its sessions are IBgpPeer (self is not a route reflector nor a boundary router)
            for (_, session_type) in &router.bgp_sessions {
                match *session_type {
                    // bug persists here
                    // what about the non-route-reflector?
                    BgpSessionType::EBgp => continue 'outer,
                    _ => {}
                }
            }
            // a fifo to store the current devices
            let mut level = vec![*id];
            let mut zone = HashSet::<RouterId>::new();
            // evaluates only when it is an internal device
            while level.len() != 0 {
                let current_id = level.remove(0);
                if !zone.contains(&current_id) {
                    if let NetworkDevice::InternalRouter(current_router) = net.get_device(current_id) {
                        zone.insert(current_id);
                        // test if self is a boundary router
                        let is_boundary = current_router.bgp_sessions
                            .iter()
                            .map(|(_, session)| *session == BgpSessionType::EBgp)
                            .fold(false, |acc, x| acc | x);
                        if !is_boundary {
                            // add all IBgpPeers
                            for (id, session) in &current_router.bgp_sessions {
                                match *session {
                                    BgpSessionType::IBgpPeer => level.push(*id),
                                    _ => {}
                                }
                            }
                        }
                        
                    }
                }
            }
            // push the current zone into the final collection of zones
            zones.insert(*id, zone);
        }
    }
    // for each internal router, reversely find its parents and grandparents
    zones
}

fn bind_config_to_zone() {

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
        println!("{:?}", map);
    }
    
    #[test]
    fn test_bipartite_gadget_partition() {
        let net = example_networks::BipartiteGadget::<Repetition5>::net(2);
        let map = strategy_zone::zone_partition(&net);
        println!("{:?}", map);
    }
}