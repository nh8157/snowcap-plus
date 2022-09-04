use crate::dep_groups::strategy_trta::StrategyTRTA;
use crate::dep_groups::utils::*;
use crate::hard_policies::{Condition, HardPolicy};
use crate::netsim::config::{ConfigExpr, ConfigModifier};
use crate::netsim::types::Destination::*;
use crate::netsim::{BgpSessionType, ForwardingState, Network, NetworkError, Prefix, RouterId};
use crate::parallelism::{ConfigId, Dag, SolutionBuilder};
use crate::strategies::{Strategy, StrategyDAG};
use crate::{Error, Stopper};
use std::collections::{HashMap, HashSet};
use std::iter::zip;
use std::time::Duration;
use crate::netsim::ospfzone::find_ospf_strict_zone;

pub(crate) type ZoneId = RouterId;

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: ZoneId,                 // Identifier of the current zone
    pub routers: HashSet<RouterId>, // Routers that belong to this zone
    pub configs: Vec<ConfigId>,     // Configurations that belong to this zone
    ordering: Option<Vec<ConfigModifier>>,
    hard_policy: HashSet<Condition>,
    virtual_boundary_routers: HashSet<RouterId>,
    emulated_network: Network,
}

impl Zone {
    pub fn new(id: RouterId, net: &Network) -> Self {
        Self {
            id: id,
            routers: HashSet::new(),
            configs: Vec::new(),
            ordering: None,
            hard_policy: HashSet::new(),
            virtual_boundary_routers: HashSet::new(),
            emulated_network: net.to_owned(),
        }
    }

    fn contains_router(&self, router: &RouterId) -> bool {
        self.routers.contains(router)
    }

    fn set_routers(&mut self, routers: Vec<RouterId>) {
        routers.iter().for_each(|r| self.add_router(*r));
    }

    fn add_router(&mut self, router: RouterId) {
        self.routers.insert(router);
    }

    fn add_config(&mut self, config: usize) {
        self.configs.push(config);
    }

    fn add_hard_policy(&mut self, condition: Condition) {
        if !self.hard_policy.contains(&condition) {
            self.hard_policy.insert(condition);
        }
    }

    fn _get_id(&self) -> RouterId {
        return self.id;
    }

    fn get_routers(&self) -> HashSet<RouterId> {
        self.routers.clone()
    }

    fn get_configs(&self) -> Vec<usize> {
        self.configs.clone()
    }

    fn get_ordering(&self) -> Option<Vec<ConfigModifier>> {
        self.ordering.clone()
    }

    fn get_virtual_boundary_routers(&self) -> HashSet<RouterId> {
        self.virtual_boundary_routers.clone().into_iter().collect()
    }

    fn get_policy(&self) -> Vec<Condition> {
        self.hard_policy.clone().into_iter().collect()
    }

    fn get_emulated_network(&self) -> &Network {
        &self.emulated_network
    }

    // This function takes in an array of configurations, filter out the irrelevant configurations,
    // pass the relevant configs into strartegy_trta, map the configs output to their ids
    fn solve_ordering(&mut self, configs: &Vec<ConfigModifier>) -> Result<(), Error> {
        let relevant_configs = self.map_idx_to_config(configs)?;
        // Convert the conditions into hard policy object?
        let mut strategy = StrategyTRTA::new(
            self.get_emulated_network().to_owned(),
            relevant_configs.clone(),
            HardPolicy::globally(self.get_policy()),
            None,
        )?;
        self.ordering = Some(strategy.work(Stopper::new())?);
        // self.emulated_network.undo_action();
        Ok(())
    }

    fn map_idx_to_config(
        &self,
        configs: &Vec<ConfigModifier>,
    ) -> Result<Vec<ConfigModifier>, Error> {
        let mut relevant_configs = Vec::new();
        for config in configs.iter() {
            temp_config = match config{
                ConfigModifier::Insert(e) => e,
                ConfigModifier::Remove(e) => e,
                ConfigModifier::Update(to,..) => to,
            }
            match temp_config{
                ConfigExpr::IgpLinkWeight => {
                    if self.get_routers.iter().any(|&temp_router|,temp_router==config.source) && self.get_routers.iter().any(|&temp_router|,temp_router==config.target){
                        relevant_configs.push(config.to_owned());
                    }
                }
                _ => {relevant_configs.push(config.to_owned())};
            }
        }
        Ok(relevant_configs)
    }

pub fn merge_zone_order(order_results:Vec<Option<Vec<ConfigModifier>>>) -> Vec<ConfigModifier>{
    let mut final_ordering:Vec<ConfigModifier>=Vec::new();
    for packed_suborder in order_results.iter() {
        let mut unpacked_suborder:Vec<ConfigModifier> = packed_suborder.unwrap();
        let mut temp_index=-1;
        for temp_router in unpacked_suborder.iter(){
            if final_ordering.iter().any(|&ordered_router|,ordered_router==temp_router){
                temp_index=final_ordering.iter().position(|&index_router|,index_router==temp_router).unwrap();
            }
            else{
                temp_index=temp_index+1;
                final_ordering.insert(temp_index,temp_router.to_owned());
            }
        }
    }
    final_ordering
}