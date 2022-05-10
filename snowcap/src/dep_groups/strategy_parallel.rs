use itertools::{Itertools, fold};
use petgraph::csr::NodeIndex;

use crate::netsim::types::{Destination, Prefix, Zone};
use crate::netsim::{Network, RouterId, ForwardingState};
use crate::netsim::config::{ConfigModifier, Config, ConfigExpr};
use crate::netsim::config::ConfigExpr::*;
use crate::strategies::StrategyDAG;
use crate::hard_policies::{HardPolicy, PolicyError, Condition};
use crate::{Error, Stopper, error};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{SystemTime, Duration, Instant};
use log::error;
use thiserror::Error;
use daggy::Dag;

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
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    stop_time: Option<Duration>
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
        
        let strategy = Box::new(
            Self {
                net: net,
                dag: Dag::<ConfigModifier, u32, u32>::new(),
                modifiers: modifiers,
                hard_policy: hard_policy,
                stop_time: time_budget
            }
        );
        Ok(strategy)
    }

    fn work(
        &mut self,
        abort: Stopper
    ) -> Result<Dag<ConfigModifier, u32, u32>, Error> {
        // create a new network object, apply all configurations
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
                .collect();
            config_map.insert((src, dst, reachability), relevant_configs);
        }

        // order configurations for each reachability condition
        for ((src, dst, reachability), configs) in config_map.iter() {
            let (old_route, new_route) = (
                    self.net.get_route(*src, *dst).unwrap(),
                    after_net.get_route(*src, *dst).unwrap()
            );
            let boundary_routers = StrategyParallel::find_boundary_routers(&old_route, &new_route);
            // examine reachability within each zone after the update
            // group configurations into according to zones
            for br in 0..(boundary_routers.len() - 1) {
                let zone = (boundary_routers[br], boundary_routers[br + 1]);
                let (old_zone_routers, new_zone_routers) = (
                    StrategyParallel::get_routers_in_zone(zone, &old_route).unwrap(),
                    StrategyParallel::get_routers_in_zone(zone, &new_route).unwrap()
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
        if route1.len() == 0 || route2.len() == 0 {
            return vec![];
        }
        let new_route1 = &route1[1..(route1.len())].to_vec();
        let new_route2 = &route2[1..(route2.len())].to_vec();
        if route1[0] == route2[0] {
            let mut succ = StrategyParallel::find_common_routers(new_route1, new_route2);
            succ.insert(0, route1[0]);
            return succ;
        } else {
            let (succ1, succ2) = (
                StrategyParallel::find_common_routers(route1, new_route2),
                StrategyParallel::find_common_routers(new_route1, route2)
            );
            if succ1.len() > succ2.len() {
                return succ1;
            } else {
                return succ2;
            }
        }
    }

    // these routers partition the network into different reachability zones
    fn find_boundary_routers(old_route: &Vec<RouterId>, new_route: &Vec<RouterId>) -> Vec<RouterId> {
        let common_routers = StrategyParallel::find_common_routers(old_route, new_route);
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
        if *zones.last().unwrap() != *common_routers.last().unwrap() {
            zones.push(*common_routers.last().unwrap());
        }
        zones
    }

    fn find_dependency_in_zone(
        old_route: Vec<RouterId>,
        new_route: Vec<RouterId>,
        modifiers: Vec<&ConfigModifier>,
        reachability: bool
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

    fn is_relevant_to_reachability(
        src: RouterId, 
        dst: Prefix, 
        route1: Vec<RouterId>, 
        route2: Vec<RouterId>,
        config: &ConfigModifier
    ) -> bool {
        true
    }
}

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

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_parallel::{StrategyParallel, DAG, DAGNode};
    use crate::hard_policies::HardPolicy;
    use crate::netsim::types::Destination;
    use crate::netsim::config::{Config, ConfigExpr, ConfigModifier};
    use crate::netsim::{Network, RouterId, AsId, Prefix};
    use crate::netsim::BgpSessionType::*;
    use crate::strategies::StrategyDAG;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::cell::{RefCell, Ref};
    use petgraph::graph::{NodeIndex, Node};
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
        let dag = DAG::new();
    }

    #[test]
    fn test_dag_node_add() {
        let mut node1 = Rc::new(RefCell::new(DAGNode::new()));
        let mut node2 = Rc::new(RefCell::new(DAGNode::new()));
        let mut node3 = Rc::new(RefCell::new(DAGNode::new()));
        let mut node4 = Rc::new(RefCell::new(DAGNode::new()));
        let mod1 = ConfigModifier::Insert(
            ConfigExpr::StaticRoute { router: NodeIndex::new(1), prefix: Prefix(0), target: NodeIndex::new(5) }
        );
        let mod2 = ConfigModifier::Insert(
            ConfigExpr::AccessControl { router: NodeIndex::new(1), accept: vec![], deny: vec![] }
        );
        let mod3 = ConfigModifier::Insert(
            ConfigExpr::StaticRoute { router: NodeIndex::new(4), prefix: Prefix(0), target: NodeIndex::new(5) }
        );
        assert!(node2.borrow_mut().set_val(mod1).is_ok(), true);
        assert!(node3.borrow_mut().set_val(mod2).is_ok(), true);
        assert!(node4.borrow_mut().set_val(mod3).is_ok(), true);
        assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_ok(), true);
        assert!(node2.borrow_mut().add_prev(Rc::clone(&node1)).is_ok(), true);
        assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_ok(), false);
        // assert!(node1.borrow_mut().add_next(Rc::clone(&node2)).is_err(), true);
        assert!(node1.borrow_mut().add_next(Rc::clone(&node3)).is_ok(), true);
        assert!(node3.borrow_mut().add_next(Rc::clone(&node1)).is_ok(), true); 
        assert!(node2.borrow_mut().add_next(Rc::clone(&node4)).is_ok(), true);
        assert!(node4.borrow_mut().add_prev(Rc::clone(&node2)).is_ok(), true);
        assert!(node1.borrow_mut().rmv_next(Rc::clone(&node2)).is_ok(), true);
        assert!(node2.borrow_mut().rmv_prev(Rc::clone(&node1)).is_ok(), true);
        // assert!(node2.borrow_mut().add_next(next_node))
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
}