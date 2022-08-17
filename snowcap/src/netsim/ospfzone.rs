use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use petgraph::algo::{all_simple_paths,has_path_connecting,connected_components};
use petgraph::graph;
use std::collections::{HashMap,HashSet};
use crate::netsim::{
    AsId, ConfigError, ForwardingState, LinkWeight, NetworkError, Prefix, RouterId,IgpNetwork
};
use petgraph::stable_graph;
pub fn find_ospf_strict_zone (network:&Network)-> Vec<Vec<RouterId>>{
    fn judge(router1:RouterId,router2:RouterId, graph:&IgpNetwork)->bool{
        println!("Judge: Router1 is {}. Router2 is {}.",router1.index(),router2.index());
        let mut temp_edge= graph.find_edge(router1,router2);
        if temp_edge==None{
            println! ("It is None!");
        }
        else{
            println!("{}",graph.edge_weight(temp_edge.unwrap()).unwrap());
        }
        //let connect_judge= has_path_connecting(graph,router1,router2,None);
        //if connect_judge==true{
            //println!("It is true that Router {} connects to Router {}",router1.index(),router2.index());
        //}
        //let groups = connected_components(&graph);
        //println!("Has {} group",groups);
        let possible_paths=all_simple_paths(&graph, router1, router2,0,None);
        let possible_paths_vec:Vec<HashSet<RouterId>>=possible_paths.collect();
        if possible_paths_vec.len()<2{
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
            for temp_router in &temp_intersect{
                println!("{} before intersect with path {}",temp_router.index(),path_cnt);
            }
            let this_intersection=temp_intersect.intersection(&path);
            let mut next_intersect=HashSet::new();
            for temp_router in this_intersection{
                next_intersect.insert(*temp_router);
            }
            for temp_router in &next_intersect{
                println!("{} after intersect with path {}",temp_router.index(),path_cnt);
            }
            println!("");
            temp_intersect=next_intersect;
        }
        temp_intersect.remove(&router1);
        temp_intersect.remove(&router2);
        let final_size=temp_intersect.len();
        if final_size>0{
            //if final_size==2{
                //return true;
            //}
            return false;
        }
        else{
            return true;
        }
    }

    let mut zones:Vec<Vec<RouterId>>=Vec::new();
    let mut temp_outside:Vec<RouterId>=network.get_routers();
    let mut zone_cnt=0;
    let my_network=network.clone();
    let graph=my_network.get_topology();
    while temp_outside.len()>0{
        zone_cnt=zone_cnt+1;
        let mut next_outside: Vec<RouterId>=Vec::new();
        let router1=temp_outside[0];
        let mut temp_zone:Vec<RouterId>=Vec::new();
        temp_zone.push(router1);
        for router2 in temp_outside.iter(){
            if router2.index()!=router1.index(){
                let mut result = judge(router1, *router2, graph);
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
/*
pub fn find_zone_ni(network:&Network)->Vec<Vec<RouterId>>{
    fn find_zone_rec(routers:Vec<RouterId>,graph:&IgpNetwork){

    }
    let mut zones:Vec<Vec<RouterId>>==Vec::new();
    let mut temp_outside:Vec<RouterId>=network.get_routers();
    let ori_graph=network.get_topology();
    while temp_outside.len()>0{
        let new z
    }

}
*/