use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::all_simple_paths;
use std::collections::{HashMap,HashSet};
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,IgpNetwork
};
pub fn find_ospf_strict_zone (network:&Network)-> Vec<Vec<RouterId>>{
    fn judge(router1:RouterId,router2:RouterId, graph:&IgpNetwork)->bool{
        println!("Judge: Router1 is {}. Router2 is {}.",router1.index(),router2.index());
        let possible_paths=all_simple_paths(graph, router1, router2,0,None);
        let (size,trash) = possible_paths.size_hint();
        let possible_paths_vec:Vec<HashSet<RouterId>>=possible_paths.collect();
        if size==0{
            println!("size is 0");
            return false;
        }
        let mut temp_intersect=possible_paths_vec[0].clone();
        let mut path_cnt=0;
        for path in possible_paths_vec{
            path_cnt=path_cnt+1;
            for temp_router in &path{
                println!("{} in path {}",temp_router.index(),path_cnt);
            }
            println!("");
            let this_intersection=temp_intersect.intersection(&path);
            let mut next_intersect=HashSet::new();
            for intersected_RouterId in this_intersection{
                next_intersect.insert(*intersected_RouterId);
            }
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
    let mut zone_cnt=0;
    while temp_outside.len()>0{
        zone_cnt=zone_cnt+1;
        let mut next_outside: Vec<RouterId>=Vec::new();
        let router1=temp_outside[0];
        let mut temp_zone:Vec<RouterId>=Vec::new();
        temp_zone.push(router1);
        for router2 in temp_outside.iter(){
            if router2.index()!=router1.index(){
                let mut result = judge(router1, *router2, network.get_topology());
                if result==true{
                    temp_zone.push(*router2);
                }
                else{
                    //println!("Router {} is not in zone {}",router2.index(),zone_cnt);
                    next_outside.push(*router2);
                }
            }
        }
        temp_outside=next_outside;
        zones.push(temp_zone);
    }
    return zones
}

