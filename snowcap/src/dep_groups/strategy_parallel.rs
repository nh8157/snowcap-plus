use itertools::{Itertools};

use crate::netsim::types::{Prefix, Zone};
use crate::netsim::{Network, RouterId, ForwardingState};
use crate::netsim::config::{ConfigModifier, Config, ConfigExpr};
// use crate::netsim::config::ConfigExpr::*;
use crate::strategies::StrategyDAG;
use crate::hard_policies::{HardPolicy, Condition};
use crate::{Error, Stopper};
use std::collections::HashMap;
use std::time::{Duration};
use log::error;
use daggy::{Dag, Parents, Children};
use daggy::Walker;
use std::iter;

use std::rc::Rc;
use std::cell::{RefCell};

// This is a strategy that uses the previous parallelism
// to speed up the ordering process

/// WARNING: Currently only supports ordering of access control and
/// static routes configurations under reachable and isolation condition
/// throughout the update

pub struct StrategyParallel {
    net: Network,
    dag: Dag<ConfigModifier, u32, u32>,
    dag_map: Vec<Option<daggy::NodeIndex>>,
    // use indexes to identify the location
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    _stop_time: Option<Duration>
}

impl StrategyDAG for StrategyParallel {
    fn new (
        mut net: Network,
        modifiers: Vec<ConfigModifier>,
        mut hard_policy: HardPolicy,
        time_budget: Option<Duration>
    ) -> Result<Box<Self>, Error> {
        // generates a new strategy
        hard_policy.set_num_mods_if_none(modifiers.len());
        let mut fw_state = net.get_forwarding_state();
        hard_policy.step(&mut net, &mut fw_state)?;
        
        if !hard_policy.check() {
            // error!("Initial state errors::\n{}", fmt_err(&hard_policy.get_watch_errors(), &net));
            return Err(Error::InvalidInitialState);
        }

        let dag_map: Vec<Option<daggy::NodeIndex>> = iter::repeat(None).take(modifiers.len()).collect_vec();

        let strategy = Box::new(
            Self {
                net: net,
                dag: Dag::<ConfigModifier, u32, u32>::new(),
                dag_map,
                modifiers: modifiers,
                hard_policy: hard_policy,
                _stop_time: time_budget
            }
        );
        Ok(strategy)
    }

    fn work(
        &mut self,
        _abort: Stopper
    ) -> Result<Dag<ConfigModifier, u32, u32>, Error> {
        // create a new network object, apply all configurations

        // println!("{:?}", &self.modifiers);
        let mut after_net = self.net.clone();
        for m in &self.modifiers {
            after_net.apply_modifier(m)?;
        }
        let mut before_states = ForwardingState::from_net(&self.net);
        let mut after_states = ForwardingState::from_net(&after_net);
        let mut config_map: HashMap<(RouterId, Prefix, bool), Vec<&ConfigModifier>> = HashMap::new();
        // iterate through the list of configurations and identify configurations pertinent
        // to each reachability condition
        for cond in &self.hard_policy.prop_vars {
            let (src, dst, reachability) = match cond {
                Condition::Reachable(src, prefix, None) => {
                    (*src, *prefix, true)
                },
                Condition::NotReachable(src, prefix) => {
                    (*src, *prefix, false)
                },
                _ => {
                    error!("Not yet implemented");
                    return Err(Error::NotImplemented)
                }
            };
            let relevant_configs: Vec<&ConfigModifier> = self.modifiers
                .iter()
                .filter(
                    |&x|
                    StrategyParallel::is_relevant_to_reachability(
                        src, 
                        dst, 
                        before_states.get_route(src, dst).unwrap(), 
                        after_states.get_route(src, dst).unwrap(), 
                        x
                    )
                )
                .collect_vec();
            config_map.insert((src, dst, reachability), relevant_configs);
        }

        // println!("All configs assigned");
        // println!{"{:?}", config_map};

        // order configurations for each reachability condition
        for ((src, dst, reachability), configs) in config_map.iter() {
            let (old_route, new_route) = (
                    self.net.get_route(*src, *dst).unwrap(),
                    after_net.get_route(*src, *dst).unwrap()
            );
            // println!("Initialized route");
            let boundary_routers = StrategyParallel::find_boundary_routers(&old_route, &new_route);
            // println!("{:?}", boundary_routers);
            // examine reachability within each zone after the update
            // group configurations into according to zones
            for br in 0..(boundary_routers.1.len() - 1) {
                let old_zone = (boundary_routers.0[br], boundary_routers.0[br + 1]);
                let new_zone = (boundary_routers.1[br], boundary_routers.1[br + 1]);
                // println!("Finding dependency");
                let (old_zone_routers, new_zone_routers) = (
                    StrategyParallel::get_routers_in_zone(old_zone, &old_route).unwrap(),
                    StrategyParallel::get_routers_in_zone(new_zone, &new_route).unwrap()
                );
                // configurations on the new route applied prior to configurations on old route
                // this is a dependency for the condition reachable, what about conditions that are isolate?
                // in this case, the configuration will require at least one zone to be unreachable for the traffic
                let dependency = StrategyParallel::find_dependency_in_zone(
                    old_zone_routers, 
                    new_zone_routers, 
                    configs.clone(),
                    *reachability
                ).unwrap();
                // println!("Dependency found");
                // let critical_node = self.add_node_get_index(dependency[1][0]);
                let critical_node = {
                    let index = self.modifiers.iter().position(|x| x == dependency[1][0]).unwrap();
                    if self.dag_map[index].is_none() {
                        // critical config not in dag
                        self.dag_map[index] = Some(self.dag.add_node(dependency[1][0].clone()));
                    } 
                    self.dag_map[index].unwrap()
                };
                for &c in dependency[0].iter() {
                    // check if c is in the dag
                    let index = self.modifiers.iter().position(|x| x == c).unwrap();
                    if self.dag_map[index].is_none() {
                        // c not in dag
                        let (_, node_index) = self.dag.add_parent(critical_node, 1, c.clone());
                        self.dag_map[index] = Some(node_index);
                    } else if !self.is_child(self.dag_map[index].unwrap(), critical_node) && !self.is_child(critical_node, self.dag_map[index].unwrap()) {
                        // they are parallel to each other
                        // add link from parent to child
                        self.dag.add_edge(self.dag_map[index].unwrap(), critical_node, 1).unwrap();
                    }
                }
                for &c in dependency[2].iter() {
                    let index = self.modifiers.iter().position(|x| x == c).unwrap();
                    if self.dag_map[index].is_none() {
                        // c not in dag
                        let (_, node_index) = self.dag.add_child(critical_node, 1, c.clone());
                        self.dag_map[index] = Some(node_index);
                    } else if !self.is_child(self.dag_map[index].unwrap(), critical_node) && !self.is_child(critical_node, self.dag_map[index].unwrap()) {
                        // they are parallel to each other
                        // add link from parent to child
                        self.dag.add_edge(critical_node, self.dag_map[index].unwrap(), 1).unwrap();
                    }
                }
                    // println!("{:?}", dependency);
            }
        }
        Ok(self.dag.clone())
    }
}

impl StrategyParallel {
    /// Returns whether the two routers are adjacent on a path
    fn adjacent_on_path(path: &Vec<RouterId>, r1: RouterId, r2: RouterId) -> bool {
        let pos1 = path.iter().position(|&x| x == r1).unwrap();
        let pos2 = path.iter().position(|&x| x == r2).unwrap();
        pos1 + 1 == pos2
    }

    fn get_routers_in_zone(zone: Zone, route: &Vec<RouterId>) -> Option<Vec<RouterId>> {
        let left = route.iter().position(|&x| x == zone.0).unwrap();
        let right = route.iter().position(|&x| x == zone.1).unwrap();
        if left < right {
            return Some(route[left..right].to_vec());
        }
        None
    }

    fn find_common_routers(route1: &Vec<RouterId>, route2: &Vec<RouterId>) -> Vec<RouterId> {
        let mut common = Vec::<RouterId>::new();
        let mut route_map = HashMap::<RouterId, bool>::new();
        for r in route1.iter() {
            route_map.insert(*r, false);
        }
        for r in route2.iter() {
            if route_map.contains_key(r) {
                common.push(*r as RouterId);
            }
        }
        common
        // major overhead
        // if route1.len() == 0 || route2.len() == 0 {
        //     return vec![];
        // }
        // let new_route1 = &route1[1..(route1.len())].to_vec();
        // let new_route2 = &route2[1..(route2.len())].to_vec();
        // if route1[0] == route2[0] {
        //     let mut succ = StrategyParallel::find_common_routers(new_route1, new_route2);
        //     succ.insert(0, route1[0]);
        //     return succ;
        // } else {
        //     let (succ1, succ2) = (
        //         StrategyParallel::find_common_routers(route1, new_route2),
        //         StrategyParallel::find_common_routers(new_route1, route2)
        //     );
        //     if succ1.len() > succ2.len() {
        //         return succ1;
        //     } else {
        //         return succ2;
        //     }
        // }
    }

    // these routers partition the network into different reachability zones
    fn find_boundary_routers(old_route: &Vec<RouterId>, new_route: &Vec<RouterId>) -> (Vec<RouterId>, Vec<RouterId>) {
        // println!("Finding common routers");
        // taking up most of the time
        let common_routers = StrategyParallel::find_common_routers(old_route, new_route);
        // println!("Common routers found");
        let mut zones: Vec<RouterId> = vec![common_routers[0]];
        for c in 0..(common_routers.len() - 1) {
            let this_router = common_routers[c];
            let next_router = common_routers[c + 1];
            // if in the original route the two routers form an edge, then they are not boundary routers
            if  StrategyParallel::adjacent_on_path(old_route, this_router, next_router) 
                &&
                StrategyParallel::adjacent_on_path(new_route, this_router, next_router)
            {
                continue;
            } else {
                // only add this_router if the last router added to the list is not qual to this_router
                if *zones.last().unwrap() != this_router {
                    zones.push(this_router);
                }
                zones.push(next_router);
            }
        }
        let mut new_zone = zones.clone();
        let mut old_zone = zones.clone();
        if old_route.last().unwrap() != new_route.last().unwrap() {
            // if the last hop (egress router is different)
            new_zone.push(*new_route.last().unwrap());
            old_zone.push(*old_route.last().unwrap());
        } else if *zones.last().unwrap() != *common_routers.last().unwrap() {
            new_zone.push(*common_routers.last().unwrap());
            old_zone.push(*common_routers.last().unwrap());
        }
        (old_zone, new_zone)
    }

    fn find_dependency_in_zone(
        old_route: Vec<RouterId>,
        new_route: Vec<RouterId>,
        modifiers: Vec<&ConfigModifier>,
        _reachability: bool
    ) -> Option<Vec<Vec<&ConfigModifier>>> {
        // list index represent the order
        let mut order: Vec<Vec<&ConfigModifier>> = vec![vec![], vec![], vec![]];
        for m in modifiers {
            if let Some(r) = StrategyParallel::get_target_router_from_modifier(m) {
                if r == *old_route.first().unwrap() && StrategyParallel::is_static_route(m) {
                    // this is a static route configuration on the boundary router
                    order[1].push(m);
                } else {
                    if let Some(_) = old_route.iter().position(|&x| x == r) {
                        // this is a configuration on the old route
                        order[2].push(m);
                    } else if let Some(_) = new_route.iter().position(|&x| x == r) {
                        // this is a configuration on the new route
                        order[0].push(m);
                    }
                }
            } else {
                continue;
            }
        }
        if order.iter().map(|x| x.len() > 0).fold(true, |acc, x| acc || x) {
            return Some(order);
        } 
        None
    }

    fn get_target_router_from_modifier(modifier: &ConfigModifier) -> Option<RouterId> {
        match modifier {
            ConfigModifier::Insert(c) | ConfigModifier::Remove(c) => StrategyParallel::get_target_router_from_expr(c),
            ConfigModifier::Update { from: c1, to: c2 } => {
                let r1 = StrategyParallel::get_target_router_from_expr(c1);
                let r2 = StrategyParallel::get_target_router_from_expr(c2);
                if r1.unwrap() == r2.unwrap() {
                    return r1;
                }
                None
            }
        }
    }

    fn get_target_router_from_expr(expr: &ConfigExpr) -> Option<RouterId> {
        match expr {
            // currently only support static route and access control configurations
            ConfigExpr::StaticRoute { router: r, prefix: _, target: _ } => Some(*r),
            ConfigExpr::AccessControl { router: r, accept: _, deny: _ } => Some(*r),
            _ => None
        }
    }
    
    fn is_static_route(modifier: &ConfigModifier) -> bool {
        match modifier {
            ConfigModifier::Insert(ConfigExpr::StaticRoute { router: _, prefix: _, target: _ }) |
            ConfigModifier::Update{ 
                from: ConfigExpr::StaticRoute { router: _, prefix: _, target: _ }, 
                to: ConfigExpr::StaticRoute { router: _, prefix: _, target: _ }
            } |
            ConfigModifier::Remove(ConfigExpr::StaticRoute { router: _, prefix: _, target: _ })
            => true,
            _ => false
        }
    }

    fn is_child(&self, _parent: daggy::NodeIndex, _child: daggy::NodeIndex) -> bool {
        // let children = self.dag.children(parent.into());
        // for (_, this_child) in children {
        //     if this_child == child {

        //     }
        // }
        false
    }

    fn is_relevant_to_reachability(
        _src: RouterId, 
        _dst: Prefix,
        route1: Vec<RouterId>, 
        route2: Vec<RouterId>,
        config: &ConfigModifier
    ) -> bool {
        match config {
            ConfigModifier::Insert(c) | ConfigModifier::Remove(c) => {
                match c {
                    ConfigExpr::StaticRoute { router: r1, prefix: _, target: r2 } => {
                        if  route1.iter().find(|&&x| x == *r1).is_some() && route1.iter().find(|&&x| x == *r2).is_some() ||
                            route2.iter().find(|&&x| x == *r1).is_some() && route2.iter().find(|&&x| x == *r2).is_some() {
                                return true;
                        }
                        return false;
                    }
                    _ => false
                }
            }
            ConfigModifier::Update { from: c1, to: c2 } => {
                let r1 = match c1 {
                    ConfigExpr::StaticRoute { router: r1, prefix: _, target: r2 } => {
                        route1.iter().find(|&&x| x == *r1).is_some() && route1.iter().find(|&&x| x == *r2).is_some() ||
                        route2.iter().find(|&&x| x == *r1).is_some() && route2.iter().find(|&&x| x == *r2).is_some()
                    }
                    _ => false
                };
                let r2 = match c2 {
                    ConfigExpr::StaticRoute { router: r1, prefix: _, target: r2 } => {
                        route1.iter().find(|&&x| x == *r1).is_some() && route1.iter().find(|&&x| x == *r2).is_some() ||
                        route2.iter().find(|&&x| x == *r1).is_some() && route2.iter().find(|&&x| x == *r2).is_some()
                    }
                    _ => false 
                };
                // println!("{:?}, {:?}", r1, r2);
                r1 && r2
            }
        }
    }
}

/*
Legacy code for a customized dag
Now switched to daggy::Dag
#[derive(Clone)]
pub struct DAG {
    head: Rc<RefCell<DAGNode>>,
    tail: Rc<RefCell<DAGNode>>,
    ready_tasks: Vec<Rc<RefCell<DAGNode>>>
}

impl DAG {
    fn new() -> DAG {
        let head_node = Rc::new(RefCell::new(DAGNode::new()));
        let tail_node = Rc::new(RefCell::new(DAGNode::new()));
        head_node.borrow_mut()
            .add_next(Rc::clone(&tail_node)).unwrap();
        tail_node.borrow_mut()
            .add_prev(Rc::clone(&tail_node)).unwrap();
        let ready_tasks = vec![head_node.clone()];
        // connect the head and tail
        Self {
            head: head_node,
            tail: tail_node,
            ready_tasks: ready_tasks
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct DAGNode {
    value: Option<ConfigModifier>,
    prev: Option<Vec<Rc<RefCell<DAGNode>>>>,
    next: Option<Vec<Rc<RefCell<DAGNode>>>>,
    dep: u32,
    status: bool
}

impl DAGNode {
    // initialize a new node object
    fn new() -> DAGNode {
        Self {
            value: None,
            prev: None,
            next: None,
            dep: 0,
            status: false
        }
    }
    // add a node to the list of prev
    fn add_prev(&mut self, prev_node: Rc<RefCell<DAGNode>>) -> Result<(), DAGError> {
        if self.prev.is_some() {
            // need to determine if the node is a prev of any node in prev
            if self.is_prev(&prev_node) {
                return Err(DAGError::PrevExists);
            }
            self.prev.as_mut().unwrap().push(prev_node);
        } else {
            self.prev = Some(vec![prev_node]);
        }
        Ok(())
    }
    // add a node to the list of next
    fn add_next(&mut self, next_node: Rc<RefCell<DAGNode>>) -> Result<(), DAGError> {
        if self.next.is_some() {
            if self.is_next(&next_node) {
                return Err(DAGError::NextExists);
            } 
            self.next.as_mut().unwrap().push(next_node);
        } else {
            self.next = Some(vec![next_node]);
        }
        Ok(())
    }
    // deletes immediate dependency
    fn rmv_prev(&mut self, prev_node: Rc<RefCell<DAGNode>>) -> Result<(), DAGError> {
        if let Some(i) = self.prev.clone().as_ref().unwrap().iter().position(|x| *x == prev_node) {
            self.prev.as_mut().unwrap().remove(i);
            return Ok(());
        }
        Err(DAGError::PrevNotExists)
    }
    // deletes immediate dependency
    fn rmv_next(&mut self, next_node: Rc<RefCell<DAGNode>>) -> Result<(), DAGError> {
        if let Some(i) = self.next.clone().as_ref().unwrap().iter().position(|x| *x == next_node) {
            self.next.as_mut().unwrap().remove(i);
            return Ok(());
        }
        Err(DAGError::NextNotExists)
    }
    fn print_prev(&self) {
        println!("{:?}", self.prev.clone().unwrap());
    }
    fn print_next(&self) {
        println!("{:?}", self.next.clone().unwrap());
    }
    fn set_val(&mut self, val: ConfigModifier) -> Result<(), DAGError> {
        if !self.value.is_none() {
            return Err(DAGError::ValueExists);
        }
        self.value = Some(val.clone());
        Ok(())
    }
    fn update_dep(&mut self) -> Result<bool, Error> {
        self.dep += 1;
        // this task is ready when all its dependencies are satisfied
        if self.dep as usize == self.prev.as_ref().unwrap().len() {
            self.status = true;
        }
        Ok(self.status)
    }
    // a dummy function that will always return false
    fn is_prev(&self, node: &Rc<RefCell<DAGNode>>) -> bool {
        let prev = self.prev.clone();
        match prev {
            Some(l) => {
                // if let Some(_) = l.iter().find(|&x| x == node) {
                //     return true;
                // }
                return false;
                // l.iter().map(|x| (*x).borrow_mut().is_prev(node)).fold(false, |acc, x| acc || x)
            },
            None => false
        }
    }
    // a dummy function that will always return false
    fn is_next(&self, node: &Rc<RefCell<DAGNode>>) -> bool {
        let next = self.next.clone();
        match next {
            Some(l) => {
                // Always have 'already mutably borrowed: BorrowError' here
                // if let Some(_) = l.iter().find(|&x| x == node) {
                //     return true;
                // }
                return false;
                // l.iter().map(|x| (*x).borrow_mut().is_next(node)).fold(false, |acc, x| acc || x)
            },
            None => false
        } 
    }
}

#[derive(Debug, Error)]
enum DAGError {
    #[error("Previous exists")]
    PrevExists,
    #[error("Next exists")]
    NextExists,
    #[error("Value exists")]
    ValueExists,
    #[error("Previous does not exist")]
    PrevNotExists,
    #[error("Next does not exist")]
    NextNotExists
}
*/

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_parallel::{StrategyParallel};
    use crate::hard_policies::HardPolicy;
    use crate::netsim::config::{Config, ConfigExpr, ConfigModifier};
    use crate::netsim::{Network, RouterId, AsId, Prefix};
    use crate::netsim::BgpSessionType::*;
    use crate::strategies::{StrategyDAG, Strategy};
    use crate::strategies::StrategyTRTA;
    use crate::example_networks;
    use crate::example_networks::repetitions::*;
    use crate::example_networks::ExampleNetwork;
    use crate::Stopper;
    use std::fs::File;
    use std::os::unix::prelude::FileExt;
    use std::path::Path;
    use std::time::{Instant};
    use std::collections::HashMap;
    use petgraph::graph::{NodeIndex};
    use daggy::Dag;

    #[test]
    fn test_initialize_strategy_parallel() {
        let mut net = Network::new();
        let mut conf1 = Config::new();
        let rvector: Vec<RouterId> = (0..).take(5).map(|x| net.add_router(x.to_string())).collect();
        let e: RouterId = net.add_external_router("e", AsId(100));
        net.add_link(rvector[0], e);
        // connect routers into a ring
        for r in (0..(rvector.len() - 1)) {
            let r1 = rvector[r];
            let r2 = rvector[(r + 1)];
            net.add_link(r1, r2);
            conf1.add(ConfigExpr::StaticRoute { router: r2, prefix: Prefix(0), target: r1 }).unwrap();
            if r > 0 {
                conf1.add(ConfigExpr::BgpSession { source: rvector[0], target: r1, session_type: IBgpPeer }).expect("IBGP failed");
            } else {
                conf1.add(ConfigExpr::StaticRoute { router: r1, prefix: Prefix(0), target: e }).expect("Overload");
                conf1.add(ConfigExpr::BgpSession { source: r1, target: e, session_type: EBgp }).expect("EBGP failed");
            }
        }

        net.set_config(&conf1).expect("An error occurred");

        let route = net.get_route(rvector[4], Prefix(0));

        let modifiers = vec![
            ConfigModifier::Update { 
                from: (ConfigExpr::StaticRoute { router: rvector[4], prefix: Prefix(0), target: rvector[3] }), 
                to: (ConfigExpr::StaticRoute { router: rvector[4], prefix: Prefix(0), target: rvector[0] }) 
            }
        ];
        let policies = HardPolicy::reachability(net.get_routers().iter(), net.get_known_prefixes().iter());

        let strate = StrategyParallel::new(
            net,
            modifiers,
            policies,
            None
        );
    }
    
    #[test]
    fn test_find_common_router() {
        let l1: Vec<NodeIndex> = vec![
            NodeIndex::new(1),
            NodeIndex::new(2),
            NodeIndex::new(5),
            NodeIndex::new(6)
        ];
        let l2: Vec<NodeIndex> = vec![
            NodeIndex::new(1),
            NodeIndex::new(3),
            NodeIndex::new(5),
            NodeIndex::new(6),
            NodeIndex::new(7)
        ];

        let common = StrategyParallel::find_common_routers(&l1, &l2);
        println!("{:?}", common);
    }
    #[test]
    fn test_boundary_routers() {
        let route1 = vec![
            NodeIndex::new(1), 
            NodeIndex::new(2), 
            NodeIndex::new(3), 
            NodeIndex::new(4), 
            NodeIndex::new(6)
        ];
        let route2 = vec![
            NodeIndex::new(1), 
            NodeIndex::new(3), 
            NodeIndex::new(4), 
            NodeIndex::new(5),
            NodeIndex::new(6)
        ];

        let boundary_routers = StrategyParallel::find_boundary_routers(&route1, &route2);

        println!("{:?}", boundary_routers);
    }

    #[test]
    fn test_zone_partition() {
        let old_path: Vec<NodeIndex>= vec![
            NodeIndex::new(1),
            NodeIndex::new(2),
            NodeIndex::new(3),
            NodeIndex::new(4)
        ];
        let new_path: Vec<NodeIndex> = vec![
            NodeIndex::new(1),
            NodeIndex::new(5),
            NodeIndex::new(6),
            NodeIndex::new(4)
        ];
        let mod1 = ConfigModifier::Update{
            from: ConfigExpr::StaticRoute{router: NodeIndex::new(1), prefix: Prefix(0), target: NodeIndex::new(2)},
            to: ConfigExpr::StaticRoute{router: NodeIndex::new(1), prefix: Prefix(0), target: NodeIndex::new(5)}
        };
        let mod2 = ConfigModifier::Insert( 
            ConfigExpr::StaticRoute {router: NodeIndex::new(5), prefix: Prefix(0), target: NodeIndex::new(6)}
        );
        let mod3 = ConfigModifier::Insert(
            ConfigExpr::StaticRoute { router: NodeIndex::new(6), prefix: Prefix(0), target: NodeIndex::new(4) }
        );
        let mod4 = ConfigModifier::Remove(
            ConfigExpr::StaticRoute { router: NodeIndex::new(2), prefix: Prefix(0), target: NodeIndex::new(3) }
        );
        let config_modifiers: Vec<&ConfigModifier> = vec![&mod1, &mod2, &mod3, &mod4];
        let dependency = StrategyParallel::find_dependency_in_zone(old_path, new_path, config_modifiers, true);
        println!("{:?}", dependency);
    }

    #[test]
    fn test_dag_init() {
        // let dag = DAG::new();
    }

    #[test]
    fn test_dag_node_add() {
        // let mut node1 = Rc::new(RefCell::new(DAGNode::new()));
        // let mut node2 = Rc::new(RefCell::new(DAGNode::new()));
        // let mut node3 = Rc::new(RefCell::new(DAGNode::new()));
        // let mut node4 = Rc::new(RefCell::new(DAGNode::new()));
        // let mod1 = ConfigModifier::Insert(
        //     ConfigExpr::StaticRoute { router: NodeIndex::new(1), prefix: Prefix(0), target: NodeIndex::new(5) }
        // );
        // let mod2 = ConfigModifier::Insert(
        //     ConfigExpr::AccessControl { router: NodeIndex::new(1), accept: vec![], deny: vec![] }
        // );
        // let mod3 = ConfigModifier::Insert(
        //     ConfigExpr::StaticRoute { router: NodeIndex::new(4), prefix: Prefix(0), target: NodeIndex::new(5) }
        // );
        // assert!(node2.borrow_mut().set_val(mod1).is_ok(), true);
        // assert!(node3.borrow_mut().set_val(mod2).is_ok(), true);
        // assert!(node4.borrow_mut().set_val(mod3).is_ok(), true);
        // assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_ok(), true);
        // assert!(node2.borrow_mut().add_prev(Rc::clone(&node1)).is_ok(), true);
        // assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_ok(), false);
        // // assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_err(), true);
        // assert!(node1.borrow_mut().add_next(Rc::clone(&node3)).is_ok(), true);
        // assert!(node3.borrow_mut().add_next(Rc::clone(&node1)).is_ok(), true); 
        // assert!(node2.borrow_mut().add_next(Rc::clone(&node4)).is_ok(), true);
        // assert!(node4.borrow_mut().add_prev(Rc::clone(&node2)).is_ok(), true);
        // assert!(node1.borrow_mut().rmv_next(Rc::clone(&node2)).is_ok(), true);
        // assert!(node2.borrow_mut().rmv_prev(Rc::clone(&node1)).is_ok(), true);
        // // assert!(node2.borrow_mut().add_next(next_node))
    }

    #[test]
    fn test_daggy() {
        let mod1 = ConfigModifier::Insert(
            ConfigExpr::StaticRoute { router: NodeIndex::new(1), prefix: Prefix(0), target: NodeIndex::new(5) }
        );
        let mod2 = ConfigModifier::Insert(
            ConfigExpr::AccessControl { router: NodeIndex::new(1), accept: vec![], deny: vec![] }
        );
        let mod3 = ConfigModifier::Insert(
            ConfigExpr::StaticRoute { router: NodeIndex::new(4), prefix: Prefix(0), target: NodeIndex::new(5) }
        ); 
        let mut dag = Dag::<ConfigModifier, u32, u32>::new();
        let mut dag_map: HashMap<daggy::NodeIndex, &ConfigModifier> = HashMap::new();
        let n1 = dag.add_node(mod1.clone());
        dag_map.insert(n1, &mod1);
        let n2 = dag.add_node(mod1.clone());
        let e = dag.add_edge(n1, n2, 1).unwrap();
    }


    fn get_network(repetitions: u32) -> Option<(Network, Config, HardPolicy)> {
        match repetitions {
            1 => {
                type CurrentNet = example_networks::ChainGadget<Repetition1>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            2 => {
                type CurrentNet = example_networks::ChainGadget<Repetition2>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            3 => {
                type CurrentNet = example_networks::ChainGadget<Repetition3>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            4 => {
                type CurrentNet = example_networks::ChainGadget<Repetition4>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            5 => {
                type CurrentNet = example_networks::ChainGadget<Repetition5>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            6 => {
                type CurrentNet = example_networks::ChainGadget<Repetition6>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            7 => {
                type CurrentNet = example_networks::ChainGadget<Repetition7>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            8 => {
                type CurrentNet = example_networks::ChainGadget<Repetition8>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            9 => {
                type CurrentNet = example_networks::ChainGadget<Repetition9>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            10 => {
                type CurrentNet = example_networks::ChainGadget<Repetition10>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            11 => {
                type CurrentNet = example_networks::ChainGadget<Repetition11>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            12 => {
                type CurrentNet = example_networks::ChainGadget<Repetition12>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            13 => {
                type CurrentNet = example_networks::ChainGadget<Repetition13>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            14 => {
                type CurrentNet = example_networks::ChainGadget<Repetition14>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            15 => {
                type CurrentNet = example_networks::ChainGadget<Repetition15>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            16 => {
                type CurrentNet = example_networks::ChainGadget<Repetition16>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            17 => {
                type CurrentNet = example_networks::ChainGadget<Repetition17>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            18 => {
                type CurrentNet = example_networks::ChainGadget<Repetition18>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            19 => {
                type CurrentNet = example_networks::ChainGadget<Repetition19>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            20 => {
                type CurrentNet = example_networks::ChainGadget<Repetition20>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            30 => {
                type CurrentNet = example_networks::ChainGadget<Repetition30>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            40 => {
                type CurrentNet = example_networks::ChainGadget<Repetition40>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            50 => {
                type CurrentNet = example_networks::ChainGadget<Repetition50>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            60 => {
                type CurrentNet = example_networks::ChainGadget<Repetition60>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            70 => {
                type CurrentNet = example_networks::ChainGadget<Repetition70>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            80 => {
                type CurrentNet = example_networks::ChainGadget<Repetition80>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            90 => {
                type CurrentNet = example_networks::ChainGadget<Repetition90>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            100 => {
                type CurrentNet = example_networks::ChainGadget<Repetition100>;
                let initial_variant = 0;
                let final_variant = 0;
                let net = CurrentNet::net(initial_variant);
                Some((
                    net.clone(),
                    CurrentNet::final_config(&net, final_variant),
                    CurrentNet::get_policy(&net, final_variant),
                ))
            }
            _ => None
        }
    }
    
    #[test]
    fn test_strategy_parallel() {
        type CurrentNet = example_networks::ChainGadget<Repetition100>;
        let mut net = CurrentNet::net(0);
        let mods = CurrentNet::final_config(&net, 0);
        let policy = CurrentNet::get_policy(&net, 0);
        let stopper = Stopper::new();
        let strategy = StrategyParallel::synthesize(net, mods, policy, None, stopper).unwrap();
    }

    #[test]
    fn strategy_parallel_evaluation() {
        for i in (1..51) {
            let dir = format!("../eval_parallel/parallel_evaluation_{}.txt", i);
            let path = Path::new(&dir);
            let file  = File::create(path).expect("Cannot create file");
            let mut final_string: String = String::new();
            for i in (1..101) {
                if let Some((net, configs, policy)) = get_network(i) {
                    println!("Iteration {}", i);
                    let start = Instant::now();
                    let mut iter_str: String = i.to_string();
                    let strategy = StrategyParallel::synthesize(net, configs, policy, None, Stopper::new());
                    let end = start.elapsed().as_millis().to_string();
                    // let end: String = DurationString::from(start.elapsed().as_millis()).into();
                    iter_str.push_str(" ");
                    iter_str.push_str(&end);
                    iter_str.push_str("\n");
                    final_string.push_str(&iter_str);
                    println!("Done after {:?}", end);
                }
            }
            file.write_at(final_string.as_bytes(), 0).expect("Write failed");
        }
    }

    #[test]
    fn strategy_trta_evaluation() {
        for i in (1..51) {
            let dir = format!("../eval_parallel/trta_evaluation_{}.txt", i);
            let path = Path::new(&dir);
            let file = File::create(path).expect("Cannot create file");
            let mut final_string: String = String::new();
            for i in (1..101) {
                if let Some((net, configs, policy)) = get_network(i) {
                    println!("Iteration {}", i);
                    let mut iter_str: String = i.to_string();
                    let start = Instant::now();
                    let strategy = StrategyTRTA::synthesize(net, configs, policy, None, Stopper::new());
                    let end = start.elapsed().as_millis().to_string();
                    iter_str.push_str(" ");
                    iter_str.push_str(&end);
                    iter_str.push_str("\n");
                    final_string.push_str(&iter_str);
                    // let str_to_write = iter_st
                    // file.write_at(end.as_bytes(), 32*ctr as u64).expect("Cannot write to file");
                    // file.write_at(b"\n", 34*ctr as u64).expect("Cannot write to file");
                    println!("Done after {:?}", end);
                }
            }
            file.write_at(final_string.as_bytes(), 0).expect("Write failed");
        }
    }

}