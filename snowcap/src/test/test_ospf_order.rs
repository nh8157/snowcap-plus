use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::all_simple_paths;
use std::collections::{HashMap,HashSet};
use crate::netsim::ospfzone::find_ospf_strict_zone;
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,
};
use crate::dep_groups::strategy_ospfzone::{Zone,merge_zone_order};

#[test]
fn test_ospf_ordering(){
    let mut this_network=Network::new();
    let a_id = this_network.add_router("A");
    let b_id = this_network.add_router("B");
    let c_id = this_network.add_router("C");
    let d_id = this_network.add_router("D");
    let e_id = this_network.add_router("E");
    let f_id = this_network.add_router("F");
    let g_id = this_network.add_router("G");
    this_network.add_link(a_id,b_id);
    this_network.add_link(a_id,c_id);
    this_network.add_link(d_id,b_id);
    this_network.add_link(d_id,c_id);
    this_network.add_link(d_id,e_id);
    this_network.add_link(d_id,f_id);
    this_network.add_link(f_id,g_id);
    this_network.add_link(e_id,g_id);
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: b_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: b_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: e_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: e_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: f_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: f_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: f_id, target: g_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: g_id, target: f_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: e_id, target: g_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: g_id, target: e_id, weight: 10.0 }).unwrap();
    this_network.set_config(&c).unwrap();
    let mut zones=find_ospf_strict_zone(&this_network);
    let mut zone_cnt=1;
    for zone in zones{
        for router in zone{
            println!("Router {} in zone {}",router.index(),zone_cnt);
        }
        zone_cnt=zone_cnt+1;
    }

}