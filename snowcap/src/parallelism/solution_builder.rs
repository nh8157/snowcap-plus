/*
Functionalities
    1. Log dependency between nodes
    2. Given the combination of configuration orderings and on which configuration
    does certain next hop changes, reassemble a dependency object (DAG)
    3. When synthesis has finished, return an executor object; given a network object,
    the executor object can apply changes to the network
*/

use crate::netsim::{RouterId, Prefix};
use crate::parallelism::{
    types::{ConfigId, DagError},
    Dag,
};
use std::collections::HashMap;

/// This is a tool for building dependency graph
#[derive(Debug)]
pub struct SolutionBuilder {
    node_dependency: Dag<RouterId>,
    config_dependency: Dag<ConfigId>,
    cache: HashMap<RouterId, ConfigId>, // Assuming only one configuration can change the next hop on a router
}

impl SolutionBuilder {
    pub(crate) fn new() -> Self {
        Self { node_dependency: Dag::new(), config_dependency: Dag::new(), cache: HashMap::new() }
    }

    pub(crate) fn add_node_dependency(
        &mut self,
        from: RouterId,
        to: RouterId,
    ) -> Result<(), DagError<RouterId>> {
        self.node_dependency.insert_node(from);
        self.node_dependency.insert_node(to);
        self.node_dependency.add_dependency(from, to)?;
        Ok(())
    }

    // Configs records the routers that have changed their next hop after applying config
    // The array passed in records the routers changing their next hop upon applying config
    pub(crate) fn insert_config_ordering(
        &mut self,
        configs: &Vec<(ConfigId, Vec<(RouterId, Prefix)>)>,
    ) -> Result<(), DagError<ConfigId>> {
        // Based on all configurations passed in, how do you reconstruct cross-zone configuration dependency?
        // Maybe we can use a cache object to temporarily store the mapping between router and configuration
        let mut prev_config: Option<ConfigId> = None;

        for (current_config, current_ts) in configs {
            self.config_dependency.insert_node(*current_config);
            current_ts.iter().for_each(|(x, _)| {
                self.cache.insert(*x, *current_config);
            });
            if prev_config.is_some() {
                self.config_dependency
                    .add_dependency(*prev_config.as_ref().unwrap(), *current_config)?;
            }
            prev_config = Some(*current_config);
        }
        Ok(())
    }

    // This function sets up dependency among configurations according to node dependency
    pub(crate) fn construct_config_dependency(&mut self) -> Result<(), DagError<ConfigId>> {
        Ok(())
    }

    pub(crate) fn get_node_dependency(&self) -> &Dag<RouterId> {
        &self.node_dependency
    }

    pub(crate) fn get_config_dependency(&self) -> &Dag<ConfigId> {
        &self.config_dependency
    }
}

#[cfg(test)]
mod test {
    use crate::parallelism::{solution_builder::SolutionBuilder, types::ConfigId};
    use petgraph::prelude::*;
    #[test]
    fn test_init() {
        let _builder = SolutionBuilder::new();
    }

    #[test]
    fn test_add_dependency_graph() {
        let node_0 = NodeIndex::new(0);
        let node_1 = NodeIndex::new(1);
        let node_2 = NodeIndex::new(2);
        let node_3 = NodeIndex::new(3);
        let mut builder = SolutionBuilder::new();
        builder.add_node_dependency(node_0, node_1).unwrap();
        builder.add_node_dependency(node_1, node_2).unwrap();
        builder.add_node_dependency(node_2, node_3).unwrap();
    }

    #[test]
    fn test_add_array_of_configs() {
        let configs: Vec<_> = (0..10).map(|x| (x as ConfigId, vec![])).collect();
        let mut builder = SolutionBuilder::new();
        builder.insert_config_ordering(&configs).unwrap();
    }
}
