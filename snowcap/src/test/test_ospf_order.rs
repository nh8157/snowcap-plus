use crate::netsim::config::{Config,ConfigExpr,ConfigModifier,ConfigPatch};
use crate::netsim::router::Router;
use crate::netsim::network::Network;
use crate::hard_policies::{Condition, HardPolicy};
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
    let mut c = Config::new();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: b_id, weight: 8.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: a_id, weight: 8.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: a_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: a_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: b_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: b_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: c_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: c_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: e_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: e_id, target: d_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: d_id, target: f_id, weight: 8.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: f_id, target: d_id, weight: 8.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: f_id, target: g_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: g_id, target: f_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: e_id, target: g_id, weight: 10.0 }).unwrap();
    c.add(ConfigExpr::IgpLinkWeight { source: g_id, target: e_id, weight: 10.0 }).unwrap();
    let mut b_accept:Vec<RouterId> = Vec::new();
    b_accept.push(a_id);
    let mut b_deny:Vec<RouterId> = Vec::new();
    let mut f_accept:Vec<RouterId> = Vec::new();
    f_accept.push(a_id);
    let mut f_deny:Vec<RouterId> = Vec::new();
    let mut acl_b=ConfigExpr::AccessControl { router: b_id, accept: b_accept, deny: b_deny };
    c.add(acl_b);
    let mut acl_f=ConfigExpr::AccessControl { router: f_id, accept: f_accept, deny: f_deny };
    c.add(acl_f);
    this_network.set_config(&c).unwrap();
    let mut zones:Vec<Vec<RouterId>>=find_ospf_strict_zone(&this_network);
    let mut zone_cnt=1;
    for zone in &zones{
        for router in zone{
            println!("Router {} in zone {}",router.index(),zone_cnt);
        }
        zone_cnt=zone_cnt+1;
    }
    let mut policy = Condition::ReachableIGP(a_id, g_id, None);
    let mut zone1=Zone::new(1,&this_network);
    let mut zone2=Zone::new(2,&this_network);
    zone1.add_hard_policy(policy.clone());
    zone2.add_hard_policy(policy.clone());
    for temp_router in zones[0].clone().iter(){
        zone1.add_router(*temp_router);
    }
    for temp_router in zones[1].clone().iter(){
        zone2.add_router(*temp_router);
    }
    zone2.add_router(d_id);
    let mut configs:Vec<ConfigModifier> = Vec::new();
    let mut config_1=ConfigModifier::Update{from:(ConfigExpr::IgpLinkWeight{source:a_id, target:c_id, weight:10.0}),to:(ConfigExpr::IgpLinkWeight{source:a_id, target:c_id, weight:5.0})};
    let mut config_2=ConfigModifier::Update{from:(ConfigExpr::IgpLinkWeight{source:c_id, target:a_id, weight:10.0}),to:(ConfigExpr::IgpLinkWeight{source:c_id, target:a_id, weight:5.0})};
    let mut config_3=ConfigModifier::Update{from:(ConfigExpr::IgpLinkWeight{source:d_id, target:e_id, weight:10.0}),to:(ConfigExpr::IgpLinkWeight{source:d_id, target:e_id, weight:5.0})};
    let mut config_4=ConfigModifier::Update{from:(ConfigExpr::IgpLinkWeight{source:e_id, target:d_id, weight:10.0}),to:(ConfigExpr::IgpLinkWeight{source:e_id, target:d_id, weight:5.0})};
    let mut old_b_accept:Vec<RouterId> = Vec::new();
    old_b_accept.push(a_id);
    let mut old_b_deny:Vec<RouterId> = Vec::new();
    let mut new_b_accept:Vec<RouterId> = Vec::new();
    let mut new_b_deny:Vec<RouterId> = Vec::new();
    new_b_deny.push(a_id);
    let mut config_5=ConfigModifier::Update{from:(ConfigExpr::AccessControl { router: b_id, accept: old_b_accept, deny: old_b_deny }),to:(ConfigExpr::AccessControl { router: b_id, accept: new_b_accept, deny: new_b_deny })};
    let mut old_f_deny:Vec<RouterId> = Vec::new();
    let mut old_f_accept:Vec<RouterId> = Vec::new();
    old_f_accept.push(a_id);
    let mut new_f_accept:Vec<RouterId> = Vec::new();
    let mut new_f_deny:Vec<RouterId> = Vec::new();
    new_f_deny.push(a_id);
    let mut config_6=ConfigModifier::Update{from:(ConfigExpr::AccessControl { router: f_id, accept: old_f_accept, deny: old_f_deny }),to:(ConfigExpr::AccessControl { router: f_id, accept: new_f_accept, deny: new_f_deny })};

    configs.push(config_1);
    configs.push(config_2);
    configs.push(config_3);
    configs.push(config_4);
    configs.push(config_5);
    configs.push(config_6);
    let mut ordering_1 = zone1.solve_ordering(&configs).ok();
    let mut ordering_2 = zone2.solve_ordering(&configs).ok();
    //let mut final_order_set = Vec::new();
    //println! ("order1 has length: {}",ordering_1.unwrap().length());
    //final_order_set.push(ordering_1);
    //final_order_set.push(ordering_2);
    /*
    let mut final_order = merge_zone_order(final_order_set);
    for i in final_order.iter(){
        let temp_config={
            match i{
                ConfigModifier::Insert(e) => e,
                ConfigModifier::Remove(e) => e,
                ConfigModifier::Update{from,to} => to,
            }
        };
        match temp_config{
            ConfigExpr::IgpLinkWeight {source, target, weight} => {
                println! ("Link weight change. source: {}, target: {}, weight: {}", source.index(),target.index(),weight);
            },
            ConfigExpr::AccessControl {router,accept,deny} =>{
                print! ("Acl. router: {}, accept: ",router.index());
                for i in accept.iter(){
                    print! ("{}, ",i.index());
                }
                print! ("deny: ");
                for i in deny.iter(){
                    print! ("{}, i.index()");
                }
            }
            _ => {println! ("No!");}
        }
    }
    */
}