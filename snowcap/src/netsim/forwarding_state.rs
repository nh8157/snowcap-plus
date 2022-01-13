// Snowcap: Synthesizing Network-Wide Configuration Updates
// Copyright (C) 2021  Tibor Schneider
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, write to the Free Software Foundation, Inc.,
// 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

//! # This module contains the implementation of the global forwarding state. This is a structure
//! containing the state, and providing some helper functions to extract certain information about
//! the state.

use crate::netsim::{Network, NetworkDevice, NetworkError, Prefix, RouterId};
use crate::netsim::config::{Config, ConfigExpr};
use crate::netsim::types::{Destination, ACL};
use log::*;
use std::collections::{HashMap, HashSet};
use std::iter::{repeat, Peekable, FromIterator};
use std::vec::IntoIter;

use super::router::Router;

/// # Forwarding State
///
/// This is a structure containing the entire forwarding state. It provides helper functions for
/// quering the state to get routes, and other information.
///
/// We use indices to refer to specific routers (their ID), and to prefixes. This improves
/// performance. However, we know that the network cannot delete any router, so the generated
/// routers will have monotonically increasing indices. Thus, we simply use that.
///
/// In addition, the `ForwardingState` caches the already computed results of any path for faster
/// access.
#[derive(Debug, Clone)]
pub struct ForwardingState {
    /// Number of prefixes, needed for computing the index
    num_prefixes: usize,
    /// Number of routers, needed to check if the router exists
    num_devices: usize,
    /// Flattened 2-dimensional vector for the routers, the prefixes, and the rest of the routers. 
    /// The value is None if the router knows no route ot the prefix, and the value is Some(usize)
    /// with usize being the index
    /// to the `RouterId`.
    state: Vec<Option<RouterId>>,
    /// Storing ACL rules of all routers
    acl: Vec<Option<(ACL, HashSet<RouterId>)>>,
    /// Lookup for the Prefix
    pub(self) prefixes: HashMap<Prefix, usize>,
    /// Lookup for IGP routers
    pub(self) routers: Vec<RouterId>,
    /// lookup to tell which routers are external
    external_routers: HashSet<RouterId>,
    /// Cache storing the result from the last computation. The outer most vector is the corresponds
    /// to the router id, and the next is the prefix. Then, if cache[r * num_prefixes + p] is None,
    /// we have not yet computed the result there, But if cache[r * num_prefixes + p] is true, then
    /// it will store the result which was computed last time.
    cache: Vec<Option<(CacheResult, Vec<RouterId>)>>,
}

impl PartialEq for ForwardingState {
    fn eq(&self, other: &Self) -> bool {
        if self.num_prefixes != other.num_prefixes || self.num_devices != other.num_devices {
            return false;
        }

        for prefix in self.prefixes.keys() {
            for rid in 0..self.num_devices {
                let router = (rid as u32).into();
                if self.get_next_hop(router, *prefix) != other.get_next_hop(router, *prefix) {
                    return false;
                }
            }
        }
        true
    }
}

impl ForwardingState {
    /// Extracts the forwarding state from the network.
    pub fn from_net(net: &Network) -> Self {
        let num_devices = net.num_devices();

        // initialize the prefix lookup
        let prefixes = net
            .get_known_prefixes()
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, i))
            .collect::<HashMap<Prefix, usize>>();
        let num_prefixes = prefixes.len();

        // initialize another table for igp here
        let routers = net
            .get_routers()
            .clone();

        // initialize state
        // need to account for igp as well
        let mut state: Vec<Option<RouterId>> =
            repeat(None).take(num_prefixes * num_devices).collect();
        for rid in 0..num_devices as u32 {
            if let NetworkDevice::InternalRouter(r) = net.get_device(rid.into()) {
                for (p, pid) in prefixes.iter() {
                    state[get_idx(rid as usize, *pid, num_prefixes)] = r.get_next_hop(Destination::BGP(*p));
                }
            }
        }

        // collect the external routers, and chagne the forwarding state such that we remember which
        // prefix they know a route to.
        let external_routers: HashSet<RouterId> = net.get_external_routers().into_iter().collect();
        for r in external_routers.iter() {
            for p in net.get_device(*r).unwrap_external().advertised_prefixes() {
                state[get_idx(r.index(), *prefixes.get(&p).unwrap(), num_prefixes)] = Some(*r);
            }
        }

        // prepare the cache
        let cache = repeat(None).take(num_prefixes * num_devices).collect();
        let acl: Vec<Option<(ACL, HashSet<RouterId>)>> = Vec::new();
        Self { num_prefixes, num_devices, state, acl, prefixes, routers, external_routers, cache }
    }

    /// New function that returns a forwarding state object indexing IGP communication
    pub fn from_net_new(net: &Network) -> Self {
        let num_devices = net.num_devices();

        // initialize the prefix lookup
        let prefixes = net
            .get_known_prefixes()
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, i))
            .collect::<HashMap<Prefix, usize>>();
        let num_prefixes = prefixes.len();

        // initialize another table for igp here
        let routers = net.get_routers();

        // initialize state
        // need to account for igp as well
        let mut state: Vec<Option<RouterId>> =
            repeat(None).take((num_devices + num_prefixes) * num_devices).collect();
        for rid in 0..num_devices as u32 {
            if let NetworkDevice::InternalRouter(r) = net.get_device(rid.into()) {
                for (p, pid) in prefixes.iter() {
                    let idx: usize = get_idx_new(
                        r.router_id(),
                        &Destination::BGP(*p),
                        &prefixes,
                        &routers
                    );
                    state[idx] = r.get_next_hop(Destination::BGP(*p));
                }
                for r_other in &routers {
                    let idx: usize = get_idx_new(
                        r.router_id(),
                        &Destination::IGP(*r_other),
                        &prefixes,
                        &routers
                    );
                    if *r_other != r.router_id() {
                        // when self is not the destination
                        state[idx] = r.get_next_hop(Destination::IGP(*r_other));
                    } else {
                        // when self is the destination
                        state[idx] = Some(*r_other);
                    }
                }
            }
        }

        // collect the external routers, and change the forwarding state such that we remember which
        // prefix they know a route to.
        let external_routers: HashSet<RouterId> = net.get_external_routers().into_iter().collect();
        for r in external_routers.iter() {
            for p in net.get_device(*r).unwrap_external().advertised_prefixes() {
                let idx: usize = get_idx_new(
                    *r,
                    &Destination::BGP(p),
                    &prefixes,
                    &routers
                );
                state[idx] = Some(*r);
            }
        }

        // initialize ACL
        let mut acl: Vec<Option<(ACL, HashSet<RouterId>)>> = 
            repeat(None).take(num_devices).collect();
        // assign values to ACL
        for rid in 0..num_devices as u32 {
            let router = net.get_device(rid.into());
            if let NetworkDevice::InternalRouter(r) = router {
                let (mode, acl_router) = r.get_acl().unwrap();
                let idx = routers.iter().position(|rid| *rid == r.router_id()).unwrap();
                acl[idx] = Some((mode, acl_router));
            }
        }
        // prepare the cache
        let cache = repeat(None).take((num_prefixes + num_devices) * num_devices).collect();

        Self { num_prefixes, num_devices, state, acl, prefixes, routers, external_routers, cache }
    }

    /// Returns the route from the source router to a specific prefix. This function uses the cached
    /// result from previous calls to `get_route`, and updates the cache with any new insight.
    pub fn get_route(
        &mut self,
        source: RouterId,
        prefix: Prefix,
    ) -> Result<Vec<RouterId>, NetworkError> {
        // how is it different from the get_route function in netsim
        // check if the router exists
        if source.index() >= self.num_devices {
            return Err(NetworkError::DeviceNotFound(source));
        }
        // what does the pid refer to here?
        // perhaps the id of the prefix?
        let pid = self
            .prefixes
            .get(&prefix)
            .ok_or_else(|| NetworkError::ForwardingBlackHole(vec![source]))?;
        let mut visited_routers: HashSet<RouterId> = HashSet::new();
        let mut path: Vec<RouterId> = Vec::new();
        let mut current_node = source;
        let (result, mut update_cache_upto) = loop {
            let current_idx = get_idx(current_node.index(), *pid, self.num_prefixes);
            // check if the result is already cached
            match self.cache.get(current_idx).unwrap() {
                Some((result, cache_path)) => {
                    let cache_upto = path.len();
                    path.extend(cache_path);
                    break (*result, cache_upto);
                }
                None => {}
            }

            path.push(current_node);

            // check if visited
            if !visited_routers.insert(current_node) {
                break (CacheResult::ForwardingLoop, path.len());
            }

            // check if the current_node (before next_node) is internal
            let is_external = self.external_routers.contains(&current_node);

            // get the next node and handle the errors
            current_node = match self.state.get(current_idx).unwrap() {
                Some(nh) => *nh,
                None => {
                    break (CacheResult::BlackHole, path.len());
                }
            };

            // if the previous node was external, and we are still here, this means that the
            // external router knows a route to the outside. Return the correct route
            if is_external {
                break (CacheResult::ValidPath, path.len());
            }
        };
        //println!("{:?} to check an invariance", end);

        // update the cache
        // Special case for a forwarding loop, because we need to reconstruct the loop
        if result == CacheResult::ForwardingLoop && update_cache_upto == path.len() {
            // find the first position of the last element, which must occur twice
            let loop_rid = path.last().unwrap();
            let loop_pos = path.iter().position(|x| x == loop_rid).unwrap();
            let mut tmp_loop_path = path.iter().skip(loop_pos).cloned().collect::<Vec<_>>();
            for (update_id, router) in
                path.iter().enumerate().take(update_cache_upto - 1).skip(loop_pos)
            {
                self.cache[get_idx(router.index(), *pid, self.num_prefixes)] =
                    Some((result, tmp_loop_path.clone()));
                if update_id < update_cache_upto - 1 {
                    tmp_loop_path.remove(0);
                    tmp_loop_path.push(tmp_loop_path[0]);
                }
            }
            update_cache_upto = loop_pos;
        }

        // update the regular cache
        for update_id in 0..update_cache_upto {
            self.cache[get_idx(path[update_id].index(), *pid, self.num_prefixes)] =
                Some((result, path.iter().skip(update_id).cloned().collect()));
        }

        // write the debug message
        match result {
            CacheResult::ValidPath => Ok(path),
            CacheResult::BlackHole => {
                trace!("Black hole detected: {:?}", path);
                Err(NetworkError::ForwardingBlackHole(path))
            }
            CacheResult::ForwardingLoop => {
                trace!("Forwarding loop detected: {:?}", path);
                Err(NetworkError::ForwardingLoop(path))
            }
            CacheResult::AccessDenied => {
                trace!("Access denied");
                Err(NetworkError::AccessDenied(current_node))
            }
        }
    }

    /// New get_route function that supports IGP
    fn get_route_new(
        &mut self,
        src: RouterId,
        dest: Destination,
    ) -> Result<Vec<RouterId>, NetworkError> {
        if src.index() >= self.num_devices {
            return Err(NetworkError::DeviceNotFound(src));
        }
        
        let mut current_node = src;
        let mut current_idx: usize;
        let mut visited_routers: HashSet<RouterId> = HashSet::new();
        let mut path: Vec<RouterId> = Vec::new();
                
        match dest {
            Destination::IGP(r) => {
                if !self.routers.contains(&r) {
                    return Err(NetworkError::DeviceNotFound(r));
                } else {
                    // everything is fine
                }
            }
            Destination::BGP(p) => {
                if let None = self.prefixes.get(&p){
                    return Err(NetworkError::ForwardingBlackHole(vec![src]));
                } else {
                    // everything is fine
                }
            }
        }
        let (result, mut update_cache_upto) = loop {
            current_idx = get_idx_new(current_node, &dest, &self.prefixes, &self.routers);
            
            // check if the route already exists in cache
            if let Some((mut r, c)) = self.get_cache(current_node, &dest) {
                println!("Using cache");
                // test if access is accepted along the path
                for router in c {
                    path.push(router);
                    if !self.check_access(src, router) {
                        r = CacheResult::AccessDenied;
                        break;
                    } 
                }
                break (r, path.len());
            }

            path.push(current_node);

            if !visited_routers.insert(current_node) {
                // the router was visited before, forwarding loop detected
                break (CacheResult::ForwardingLoop, path.len());
            }

            // println!("At router {:?}", current_node);
            match &self.state[current_idx] {
                Some(r) => {
                    // check if access is denied here
                    if self.check_access(src, current_node) {
                        if *r == current_node {
                            // has arrived at the destination
                            break (CacheResult::ValidPath, path.len());
                        } else {
                            current_node = *r;
                        }
                    } else {
                        break (CacheResult::AccessDenied, path.len());
                    }    
                    // not intercepted by acl rules
                }
                None => break (CacheResult::BlackHole, path.len()),
            }   
        };

        match result {
            CacheResult::AccessDenied => {
                // only update the src
                self.cache[get_idx_new(src, &dest, &self.prefixes, &self.routers)] =
                    Some((result, path.clone()));
            }
            _ => {
                // records the looped part for each node along the loop
                if result == CacheResult::ForwardingLoop && update_cache_upto == path.len() {
                    // find the first position of the last element, which must occur twice
                    let loop_rid = path.last().unwrap();
                    let loop_pos = path.iter().position(|x| x == loop_rid).unwrap();
                    let mut tmp_loop_path = path.iter().skip(loop_pos).cloned().collect::<Vec<_>>();
                    for (update_id, router) in
                        path.iter().enumerate().take(update_cache_upto - 1).skip(loop_pos)
                    {
                        self.cache[get_idx_new(*router, &dest, &self.prefixes, &self.routers)] =
                            Some((result, tmp_loop_path.clone()));
                        if update_id < update_cache_upto - 1 {
                            tmp_loop_path.remove(0);
                            tmp_loop_path.push(tmp_loop_path[0]);
                        }
                    }
                    update_cache_upto = loop_pos;
                }
        
                // insert the newest path into cache
                for idx in 0..update_cache_upto {
                    self.cache[get_idx_new(path[idx], &dest, &self.prefixes, &self.routers)] = 
                        Some((result, path.iter().skip(idx).cloned().collect()));
                }
            }
        }

        match result {
            CacheResult::ValidPath => Ok(path),
            CacheResult::BlackHole => Err(NetworkError::ForwardingBlackHole(path)),
            CacheResult::ForwardingLoop => Err(NetworkError::ForwardingLoop(path)),
            CacheResult::AccessDenied => Err(NetworkError::AccessDenied(*path.last().unwrap())),
        }
    }

    /// Get the next hop of a router for a specific prefix. If that router does not know any route,
    /// `Ok(None)` is returned.
    pub fn get_next_hop(
        &self,
        router: RouterId,
        prefix: Prefix,
    ) -> Result<Option<RouterId>, NetworkError> {
        if router.index() >= self.num_devices {
            return Err(NetworkError::DeviceNotFound(router));
        }
        let pid = self.prefixes.get(&prefix);
        if let Some(pid) = pid {
            let data_idx = get_idx(router.index(), *pid, self.num_prefixes);
            Ok(*self.state.get(data_idx).unwrap())
        } else {
            Ok(None)
        }
    }

    fn get_cache(&self, src: RouterId, dest: &Destination) -> Option<(CacheResult, Vec<RouterId>)>{
        let idx = get_idx_new(src, &dest, &self.prefixes, &self.routers);
        self.cache[idx].clone()
    }

    fn check_access(&self, src: RouterId, dest: RouterId) -> bool {
        let idx = self.routers.iter().position(|rid| *rid == dest).unwrap();
        if let Some((mode, acl)) = &self.acl[idx] {
            // println!("Checking ACL on {:?}", current_node);
            // println!("ACL: {:?}", acl);
            match mode {
                ACL::Accept => {
                    if !acl.contains(&src) {
                        return false;
                    }
                }
                ACL::Deny => {
                    if acl.contains(&src) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheResult {
    ValidPath,
    BlackHole,
    ForwardingLoop,
    AccessDenied
}

fn get_idx(rid: usize, pid: usize, num_prefixes: usize) -> usize {
    // update here
    rid * num_prefixes + pid
}

/// For indexing within self.state
fn get_idx_new(src: RouterId, dest: &Destination, prefixes: &HashMap<Prefix, usize>, routers: &Vec<RouterId>) -> usize {
    let rid = routers.iter().position(|rid| *rid == src).unwrap();
    match dest {
        Destination::BGP(prefix) => {
            let pid = prefixes
                .get(prefix)
                .unwrap();
            rid * (routers.len() + prefixes.len()) + routers.len() + (*pid)
        }
        Destination::IGP(router) => {
            let rid_dest = routers.iter().position(|rid| *rid == *router).unwrap();
            rid * (routers.len() + prefixes.len()) + rid_dest
        }
    }
}

impl IntoIterator for ForwardingState {
    type Item = (RouterId, Prefix, Vec<RouterId>);
    type IntoIter = ForwardingStateIterator;

    fn into_iter(self) -> Self::IntoIter {
        let r = (0..self.num_devices)
            .map(|i| (i as u32).into())
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
        let p = self.prefixes.keys().cloned().collect::<Vec<_>>().into_iter();
        ForwardingStateIterator { fw_state: self, r, p }
    }
}

/// Iterator for iterating over every flow in the network
#[derive(Debug, Clone)]
pub struct ForwardingStateIterator {
    fw_state: ForwardingState,
    r: Peekable<IntoIter<RouterId>>,
    p: IntoIter<Prefix>,
}

impl Iterator for ForwardingStateIterator {
    type Item = (RouterId, Prefix, Vec<RouterId>);
    fn next(&mut self) -> Option<Self::Item> {
        match self.p.next() {
            Some(prefix) => {
                let router = self.r.peek()?;
                return Some((*router, prefix, self.fw_state.get_route(*router, prefix).ok()?));
            }
            None => {
                let router = self.r.next()?;
                self.p = self.fw_state.prefixes.keys().cloned().collect::<Vec<_>>().into_iter();
                let prefix = self.p.next()?;
                return Some((router, prefix, self.fw_state.get_route(router, prefix).ok()?));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::netsim::config::Config;

    use super::CacheResult::*;
    use super::*;
    #[test]
    fn test_from_net() {
        let mut net = Network::new();
        let r0 = net.add_router("r0");
        let r1 = net.add_router("r1");
        let r2 = net.add_router("r2");
        net.add_link(r0, r1);
        net.add_link(r0, r2);
        net.add_link(r1, r2);
        
        let mut config = Config::new();
        config.add(ConfigExpr::IgpLinkWeight {
            source: r0,
            target: r1,
            weight: 1.0,
        }).unwrap();
        config.add(ConfigExpr::IgpLinkWeight {
            source: r1,
            target: r2,
            weight: 1.0,
        }).unwrap();
        config.add(ConfigExpr::IgpLinkWeight {
            source: r2,
            target: r0,
            weight: 1.0,
        }).unwrap();
        config.add(ConfigExpr::AccessControl {
            router: r2,
            accept: vec![],
            deny: vec![r0],
        }).unwrap();

        net.set_config(&config).unwrap();
        let mut fw = net.get_forwarding_state_new();

        // let route1 = fw.get_route_new(r1, Destination::IGP(r2));
        let route2 = fw.get_route_new(r0, Destination::IGP(r2));
        let (r, p) = fw.get_cache(r0, &Destination::IGP(r2)).unwrap();
        let cache = fw.get_cache(r1, &Destination::IGP(r2));
        // assert_eq!(route1, Ok(vec![r1, r2]));
        assert_eq!(route2, Err(NetworkError::AccessDenied(r2)));
        assert_eq!(cache.is_none(), true);
    }
    #[test]
    fn test_route() {
        // let r0 = 0.into();
        // let r1 = 1.into();
        // let r2 = 2.into();
        // let r3 = 3.into();
        // let r4 = 4.into();
        // let r5 = 5.into();
        // let acl: Vec<Option<(ACL, HashSet<RouterId>)>> = Vec::new();
        // let routers = [r0, r1, r2, r3, r4, r5].to_vec();
        // let mut state = ForwardingState {
        //     num_prefixes: 1,
        //     num_devices: 6,
        //     state: vec![Some(r0), Some(r0), Some(r1), Some(r1), Some(r2), None],
        //     acl: acl,
        //     prefixes: maplit::hashmap![Prefix(0) => 0, ],
        //     routers,
        //     external_routers: maplit::hashset![r0, r5],
        //     cache: vec![None],
        // };
        // assert_eq!(state.get_route_new(r0, Destination::BGP(Prefix(0))), Ok(vec![r0]));
        // assert_eq!(state.get_route_new(r1, Destination::BGP(Prefix(0))), Ok(vec![r1, r0]));
        // assert_eq!(state.get_route(r2, Prefix(0)), Ok(vec![r2, r1, r0]));
        // assert_eq!(state.get_route(r3, Prefix(0)), Ok(vec![r3, r1, r0]));
        // assert_eq!(state.get_route(r4, Prefix(0)), Ok(vec![r4, r2, r1, r0]));
        // assert_eq!(
        //     state.get_route(r5, Prefix(0)),
        //     Err(NetworkError::ForwardingBlackHole(vec![r5]))
        // );
    }

    #[test]
    fn test_caching() {
        // let r0 = 0.into();
        // let r1 = 1.into();
        // let r2 = 2.into();
        // let r4 = 4.into();
        // let r5 = 5.into();
        // let mut state = ForwardingState {
        //     num_prefixes: 1,
        //     num_devices: 6,
        //     state: vec![Some(r0), Some(r0), Some(r1), Some(r1), Some(r2), None],
        //     prefixes: maplit::hashmap![Prefix(0) => 0, ],
        //     external_routers: maplit::hashset![r0, r5],
        //     cache: vec![None, None, None, None, None, None],
        // };
        // assert_eq!(state.get_route(r4, Prefix(0)), Ok(vec![r4, r2, r1, r0]));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(state.cache[4], Some((ValidPath, vec![r4, r2, r1, r0])));
        // assert_eq!(state.cache[3], None);
        // assert_eq!(state.cache[2], Some((ValidPath, vec![r2, r1, r0])));
        // assert_eq!(state.cache[1], Some((ValidPath, vec![r1, r0])));
        // assert_eq!(state.cache[0], Some((ValidPath, vec![r0])));
    }

    #[test]
    fn test_forwarding_loop_2() {
        // let r0: RouterId = 0.into();
        // //let r1: RouterId = 1.into();
        // let r2: RouterId = 2.into();
        // let r3: RouterId = 3.into();
        // let r4: RouterId = 4.into();
        // let r5: RouterId = 5.into();
        // let mut state = ForwardingState {
        //     num_prefixes: 1,
        //     num_devices: 6,
        //     state: vec![Some(r0), Some(r0), Some(r3), Some(r4), Some(r3), None],
        //     prefixes: maplit::hashmap![Prefix(0) => 0, ],
        //     external_routers: maplit::hashset![r0, r5],
        //     cache: vec![None, None, None, None, None, None],
        // };
        // assert_eq!(
        //     state.get_route(r2, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r2, r3, r4, r3]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], None);
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r3])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r3, r4])));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(
        //     state.get_route(r3, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r3, r4, r3]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], None);
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r3])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r3, r4])));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(
        //     state.get_route(r4, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r4, r3, r4]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], None);
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r3])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r3, r4])));
        // assert_eq!(state.cache[5], None);
    }

    #[test]
    fn test_forwarding_loop_3() {
        // let r0: RouterId = 0.into();
        // let r1: RouterId = 1.into();
        // let r2: RouterId = 2.into();
        // let r3: RouterId = 3.into();
        // let r4: RouterId = 4.into();
        // let r5: RouterId = 5.into();
        // let mut state = ForwardingState {
        //     num_prefixes: 1,
        //     num_devices: 6,
        //     state: vec![Some(r0), Some(r2), Some(r3), Some(r4), Some(r2), None],
        //     prefixes: maplit::hashmap![Prefix(0) => 0, ],
        //     external_routers: maplit::hashset![r0, r5],
        //     cache: vec![None, None, None, None, None, None],
        // };
        // assert_eq!(
        //     state.get_route(r1, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r1, r2, r3, r4, r2]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], Some((ForwardingLoop, vec![r1, r2, r3, r4, r2])));
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r2])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r2, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r2, r3, r4])));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(
        //     state.get_route(r2, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r2, r3, r4, r2]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], Some((ForwardingLoop, vec![r1, r2, r3, r4, r2])));
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r2])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r2, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r2, r3, r4])));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(
        //     state.get_route(r3, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r3, r4, r2, r3]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], Some((ForwardingLoop, vec![r1, r2, r3, r4, r2])));
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r2])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r2, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r2, r3, r4])));
        // assert_eq!(state.cache[5], None);
        // assert_eq!(
        //     state.get_route(r4, Prefix(0)),
        //     Err(NetworkError::ForwardingLoop(vec![r4, r2, r3, r4]))
        // );
        // assert_eq!(state.cache[0], None);
        // assert_eq!(state.cache[1], Some((ForwardingLoop, vec![r1, r2, r3, r4, r2])));
        // assert_eq!(state.cache[2], Some((ForwardingLoop, vec![r2, r3, r4, r2])));
        // assert_eq!(state.cache[3], Some((ForwardingLoop, vec![r3, r4, r2, r3])));
        // assert_eq!(state.cache[4], Some((ForwardingLoop, vec![r4, r2, r3, r4])));
        // assert_eq!(state.cache[5], None);
    }
}
