use crate::netsim::{Network, RouterId};
use crate::netsim::config::{ConfigModifier, Config};
use crate::strategies::Strategy;
use crate::hard_policies::{HardPolicy, PolicyError};
use crate::{Error, Stopper};
use std::time::{SystemTime, Duration, Instant};

use std::rc::Rc;
use std::cell::{RefCell, RefMut};

// This is a strategy that uses the previous parallelism
// to speed up the ordering process

// Currently only supports ordering of access control and
// static routes configurations

pub struct StrategyParallel {
    net: Network,
    dag: DAG,
    modifiers: Vec<ConfigModifier>,
    hard_policy: HardPolicy,
    stop_time: Option<Duration>
}

impl Strategy for StrategyParallel {
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
    ) -> Result<Vec<ConfigModifier>, Error> {
        todo!();
    }
    
    // what is this function for?
    #[cfg(feature = "count-states")]
    fn num_states(&self) -> usize {
        todo!();
    }
}

impl StrategyParallel {
    fn find_common_routers(&self) -> Option<Vec<RouterId>> {
        todo!();
    }

    fn find_zones(&self) -> Option<Vec<Vec<RouterId>>> {
        todo!();
    }

    fn find_dependency(&self, conf: Vec<ConfigModifier>) -> Option<Vec<Vec<ConfigModifier>>> {
        todo!();
    }
}

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
            .add_next(Rc::clone(&tail_node))
            .expect("Error adding tail");
        tail_node.borrow_mut()
            .add_prev(Rc::clone(&tail_node))
            .expect("Error adding head");
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
struct DAGNode {
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