use crate::netsim::config::{Config，ConfigExpr，ConfigModifier，ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::simple_paths::all_simple_paths;
use std::collections::{HashMap,HashSet};
pub fn find_OSPF_strict_zone (network:&Network)-> HashSet<RouterId>{
    fn judge(router1:RouterId,router2:RouterId, graph:&TgpNetwork, temp_outside:HashSet(RouterId>))->bool{
        let possible_paths=all_simple_paths(graph, router1, router2,0).unwrap();
        let size = possible_paths.size_hint();
        let possible_paths_vec:Vec<HashSet<RouterId>>=possible_paths.collect();
        if size==0{
            return false;
        }
        let mut temp_intersect=possible_paths_vec[0];
        for path in possible_paths {
            temp_intersect.intersection(path);
        }
        let final_size=temp_intersect.len();
        if final_size>0{
            return false;
        }
        else{
            return true;
        }
    }

    let mut zones::Vec<Vec<RouterId>>=Vec::new();
    let mut temp_outside:Vec<Routerld>=network.get_routers();
    while temp_outside.len()>0{
        let mut next_outside: Vec<RouterId>=Vec::new();
        let router1=next_outside[0];
        let mut temp_zone:Vec<Routerld>=Vec::new();
        temp_zone.push(router1)
        for router2 in next__outside.iter(){
            if router2!=router1{
                let mut result = judge (router1, router2, network.get_topology());
                if result==true{
                    temp_zone.push (router2);
                }
                else{
                    next_outside.push<router2>;
                }
            }
        }
        temp_outside=net_outside;
        zones.push(temp_zone);
    }
    return zones
}
fn main(){
    let mut this_network=Network::new();
    this_network.add_router(1);
    this_network.add_router(2);
    this_network.add_router(3);
    this_network.add_router(4);
    this_network.add_link(1,2);
    this_network.add_link(1,3);
    this_network.add_link(1,4);
    this_network.add_link(2,3);
    this_network.add_link(2,4);
    this_network.add_link(3,4);
    let mut zones=find_OSPF_strict_zone(&this_network);
    println("here!");
}
