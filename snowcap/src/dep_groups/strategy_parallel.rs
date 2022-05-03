use petgraph::csr::NodeIndex;

use crate::netsim::types::{Destination, Prefix, Zone};
use crate::netsim::{Network, RouterId, ForwardingState};
use crate::netsim::config::{ConfigModifier, Config};
use crate::netsim::config::ConfigExpr::*;
use crate::strategies::StrategyDAG;
use crate::hard_policies::{HardPolicy, PolicyError, Condition};
use crate::{Error, Stopper, error};
use std::collections::HashMap;
use std::time::{SystemTime, Duration, Instant};
use log::error;

use std::rc::Rc;
use std::cell::{RefCell};

// This is a strategy that uses the previous parallelism
// to speed up the ordering process

/// WARNING: Currently only supports ordering of access control and
/// static routes configurations under reachable and isolation condition
/// throughout the update

pub struct StrategyParallel {
    net: Network,
    dag: DAG,
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
                dag: DAG::new(),
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
    ) -> Result<DAG, Error> {
        // create a new network object, apply all configurations
        let mut after_net = self.net.clone();
        for m in &self.modifiers {
            after_net.apply_modifier(m)?;
        }
        let before_states = ForwardingState::from_net(&self.net);
        let after_states = ForwardingState::from_net(&after_net);
        let mut config_map: HashMap<(RouterId, Destination, bool), Vec<&ConfigModifier>> = HashMap::new();
        // iterate through the list of configurations and identify configurations pertinent
        // to each reachability condition
        for cond in &self.hard_policy.prop_vars {
            let (src, dst, reachability) = match cond {
                Condition::Reachable(src, prefix, None) => {
                    (*src, Destination::BGP(*prefix), true)
                },
                Condition::NotReachable(src, prefix) => {
                    (*src, Destination::BGP(*prefix), false)
                },
                _ => {
                    error!("Not yet implemented");
                    return Err(Error::NotImplemented)
                }
            };
            let relevant_configs = self.search_relevant_configs(
                src, 
                &dst,
                reachability,
                &before_states, 
                &after_states
            );
            config_map.insert((src, dst, reachability), relevant_configs);
        }

        // order configurations for each reachability condition
        for ((src, dst, reachability), configs) in config_map.iter() {
            let (old_route, new_route) = match dst {
                Destination::BGP(p) => {
                    (
                        self.net.get_route(*src, *p).unwrap(),
                        after_net.get_route(*src, *p).unwrap()
                    )
                }
                _ => todo!()
            };
            let zones = StrategyParallel::find_zones(&old_route, &new_route).unwrap();
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

    // can optimize the interface later
    fn find_zones(old_route: &Vec<RouterId>, new_route: &Vec<RouterId>) -> Option<Vec<Zone>> {
        let common_routers = StrategyParallel::find_common_routers(old_route, new_route);
        let mut zones: Vec<Zone> = Vec::new();
        for c in 0..(common_routers.len() - 1) {
            let this_router = common_routers[c];
            let next_router = common_routers[c + 1];
            // if in the original route the two routers form an edge, then they are not boundary routers
            if let true = (
                StrategyParallel::adjacent_on_path(old_route, this_router, next_router) 
                &&
                StrategyParallel::adjacent_on_path(new_route, this_router, next_router)
            ){
                continue;
            } else {
                zones.push((this_router, next_router));
            }
        }
        Some(zones)
    }

    fn find_dependency(&self, conf: Vec<ConfigModifier>) -> Option<Vec<Vec<ConfigModifier>>> {
        todo!();
    }

    fn is_relevant_to_reachability(&self, src: RouterId, dst: Destination) -> bool {
        true
    }

    fn search_relevant_configs(
        &self, 
        src: RouterId, 
        dst: &Destination,
        reachability: bool,
        before_net: &ForwardingState, 
        after_net: &ForwardingState
    ) -> Vec<&ConfigModifier> {
        
        vec![]
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

#[derive(Clone)]
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
    fn add_prev(&mut self, prev_node: Rc<RefCell<DAGNode>>) -> Result<(), Error> {
        match &mut self.prev {
            Some(lst) => lst.push(prev_node),
            None => self.prev = Some(vec![prev_node])
        }
        Ok(())
    }
    // add a node to the list of next
    fn add_next(&mut self, next_node: Rc<RefCell<DAGNode>>) -> Result<(), Error> {
        match &mut self.next {
            Some(lst) => lst.push(next_node),
            None => self.next = Some(vec![next_node])
        }
        Ok(())
    }
    fn rmv_prev(&mut self, prev_node: Box<DAGNode>) -> Result<(), Error> {
        Ok(())
    }
    fn rmv_next(&mut self, next_node: Box<DAGNode>) -> Result<(), Error> {
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
}

#[cfg(test)]
mod test {
    use crate::dep_groups::strategy_parallel::StrategyParallel;
    use crate::hard_policies::HardPolicy;
    use crate::netsim::types::Destination;
    use crate::netsim::config::{Config, ConfigExpr, ConfigModifier};
    use crate::netsim::{Network, RouterId, AsId, Prefix};
    use crate::netsim::BgpSessionType::*;
    use crate::strategies::StrategyDAG;
    use petgraph::graph::NodeIndex;

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
    fn test_find_zones() {
        let route1 = vec![
            NodeIndex::new(1), 
            NodeIndex::new(2), 
            NodeIndex::new(3), 
            NodeIndex::new(4), 
            NodeIndex::new(5), 
            NodeIndex::new(6)
        ];
        let route2 = vec![
            NodeIndex::new(1), 
            NodeIndex::new(3), 
            NodeIndex::new(4), 
            NodeIndex::new(7), 
            NodeIndex::new(6)
        ];
        let zones = StrategyParallel::find_zones(&route1, &route2);
        println!("{:?}", zones);
    }
}