use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::all_simple_paths;
use std::collections::{HashMap,HashSet};
use crate::netsim::ospfzone::find_OSPF_strict_zone;
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,
};


#[test]
fn test_ospf(){
    let mut this_network=Network::new();
    let A_id = this_network.add_router("A");
    let B_id = this_network.add_router("B");
    let C_id = this_network.add_router("C");
    let D_id = this_network.add_router("D");
    this_network.add_link(A_id,B_id);
    this_network.add_link(A_id,C_id);
    this_network.add_link(A_id,D_id);
    this_network.add_link(B_id,C_id);
    this_network.add_link(B_id,D_id);
    this_network.add_link(C_id,D_id);
    let mut zones=find_OSPF_strict_zone(&this_network);
    println!("here!");
}
