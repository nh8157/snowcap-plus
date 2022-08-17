use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::all_simple_paths;
use std::collections::{HashMap,HashSet};
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,IgpNetwork
};
pub fn find_OSPF_strict_zone (network:&Network)-> Vec<Vec<RouterId>>{
    fn judge(router1:RouterId,router2:RouterId, graph:&IgpNetwork)->bool{
        let possible_paths=all_simple_paths(graph, router1, router2,0,None);
        let (size,trash) = possible_paths.size_hint();
        let possible_paths_vec:Vec<HashSet<RouterId>>=possible_paths.collect();
        if size==0{
            return false;
        }
        let mut temp_intersect=&possible_paths_vec.clone()[0];
        for path in possible_paths_vec{
            temp_intersect.intersection(&path);
        }
        let final_size=temp_intersect.len();
        if final_size>0{
            return false;
        }
        else{
            return true;
        }
    }

    let mut zones:Vec<Vec<RouterId>>=Vec::new();
    let mut temp_outside:Vec<RouterId>=network.get_routers();
    while temp_outside.len()>0{
        let mut next_outside: Vec<RouterId>=Vec::new();
        let router1=temp_outside[0];
        let mut temp_zone:Vec<RouterId>=Vec::new();
        temp_zone.push(router1);
        for router2 in temp_outside.iter(){
            if *router2!=router1{
                let mut result = judge(router1, *router2, network.get_topology());
                if result==true{
                    temp_zone.push(*router2);
                }
                else{
                    next_outside.push(*router2);
                }
            }
        }
        temp_outside=next_outside;
        zones.push(temp_zone);
    }
    return zones
}

