use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::all_simple_paths;
use std::collections::{HashMap,HashSet};
use crate::netsim::ospfzone::find_ospf_strict_zone;
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,
};


#[test]
fn test_ospf(){
    let mut this_network=Network::new();
    let a_id = this_network.add_router("A");
    let b_id = this_network.add_router("B");
    let c_id = this_network.add_router("C");
    let d_id = this_network.add_router("D");
    this_network.add_link(a_id,b_id);
    this_network.add_link(a_id,c_id);
    this_network.add_link(a_id,d_id);
    this_network.add_link(b_id,c_id);
    //this_network.add_link(b_id,d_id);
    //this_network.add_link(c_id,d_id);
    let a_id = this_network.get_router_id("A").unwrap();
    let b_id = this_network.get_router_id("B").unwrap();
    let c_id = this_network.get_router_id("C").unwrap();
    let d_id = this_network.get_router_id("D").unwrap();
    let mut c = Config::new();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: b_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: b_id, weight: 10.0 }).unwrap();
    //c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: d_id, weight: 10.0 }).unwrap();
    //c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: b_id, weight: 10.0 }).unwrap();
    //c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: d_id, weight: 10.0 }).unwrap();
    //c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: c_id, weight: 10.0 }).unwrap();
    this_network.set_config(&c).unwrap();
    let mut zones=find_ospf_strict_zone(&this_network);
    let x = true;
    let mut zone_cnt=1;
    for zone in zones{
        for router in zone{
            println!("Router {} in zone {}",router.index(),zone_cnt);
        }
        zone_cnt=zone_cnt+1;
    }
}
