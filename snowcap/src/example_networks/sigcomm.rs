use super::ExampleNetwork;
use crate::netsim::config::{Config, ConfigExpr::*};
use crate::netsim::{AsId, Network, BgpSessionType::*, Prefix};
use crate::hard_policies::HardPolicy;

/// This is the topology present in the Sigcomm paper
pub struct Sigcomm {}

impl ExampleNetwork for Sigcomm {
    fn net(_initial_variant: usize) -> Network {
        let mut net = Network::new();
        let t1 = net.add_router("t1");
        let t2 = net.add_router("t2");
        let r1 = net.add_router("r1");
        let r2 = net.add_router("r2");
        let b1 = net.add_router("b1");
        let b2 = net.add_router("b2");
        let e1 = net.add_external_router("e1", AsId(65008));
        let e2 = net.add_external_router("e2", AsId(65009));

        net.add_link(t1, t2);
        net.add_link(b1, t1);
        net.add_link(b1, r2);
        net.add_link(b2, t2);
        net.add_link(b2, r1);
        net.add_link(r1, r2);
        net.add_link(b1, e1);
        net.add_link(b2, e2);

        let cf = Sigcomm::initial_config(&net, 0);

        net.set_config(&cf).unwrap();
        net.advertise_external_route(e1, Prefix(0), vec![AsId(65008), AsId(65020)], None, None).unwrap();
        net.advertise_external_route(e2, Prefix(0), vec![AsId(65009), AsId(65020)], None, None).unwrap();

        net
    }
    fn initial_config(net: &Network, _variant: usize) -> Config {
        let e1 = net.get_router_id("e1").unwrap();
        let e2 = net.get_router_id("e2").unwrap();
        let b1 = net.get_router_id("b1").unwrap();
        let b2 = net.get_router_id("b2").unwrap();
        let r1 = net.get_router_id("r1").unwrap();
        let r2 = net.get_router_id("r2").unwrap();
        let t1 = net.get_router_id("t1").unwrap();
        let t2 = net.get_router_id("t2").unwrap();

        let mut config = Config::new();
        config.add(
            IgpLinkWeight { source: t1, target: t2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t2, target: t1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: t1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t1, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: t2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t2, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: r1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r1, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: r2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r2, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r1, target: r2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r2, target: r1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: e1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: e1, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: e2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: e2, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            BgpSession { source: b1, target: e1, session_type: EBgp }
        ).unwrap();
        config.add(
            BgpSession { source: b2, target: e2, session_type: EBgp }
        ).unwrap();
        config.add(
            BgpSession { source: t1, target: b1, session_type: IBgpPeer }
        ).unwrap();
        config.add(
            BgpSession { source: t2, target: b2, session_type: IBgpPeer }
        ).unwrap();
        config.add(
            BgpSession { source: r1, target: b1, session_type: IBgpClient }
        ).unwrap(); 
        config.add(
            BgpSession { source: r2, target: b2, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: r1, target: b2, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: t1, target: r1, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: t2, target: r2, session_type: IBgpClient }
        ).unwrap();

        config
    }
    fn final_config(net: &Network, _variant: usize) -> Config {
        let e1 = net.get_router_id("e1").unwrap();
        let e2 = net.get_router_id("e2").unwrap();
        let b1 = net.get_router_id("b1").unwrap();
        let b2 = net.get_router_id("b2").unwrap();
        let r1 = net.get_router_id("r1").unwrap();
        let r2 = net.get_router_id("r2").unwrap();
        let t1 = net.get_router_id("t1").unwrap();
        let t2 = net.get_router_id("t2").unwrap();

        let mut config = Config::new();
        config.add(
            IgpLinkWeight { source: t1, target: t2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t2, target: t1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: t1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t1, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: t2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: t2, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: r1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r1, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: r2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r2, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r1, target: r2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: r2, target: r1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b1, target: e1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: e1, target: b1, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: b2, target: e2, weight: 1.0 }
        ).unwrap();
        config.add(
            IgpLinkWeight { source: e2, target: b2, weight: 1.0 }
        ).unwrap();
        config.add(
            BgpSession { source: b1, target: e1, session_type: EBgp }
        ).unwrap();
        config.add(
            BgpSession { source: b2, target: e2, session_type: EBgp }
        ).unwrap();
        // config.add(
        //     BgpSession { source: t1, target: b1, session_type: IBgpPeer }
        // ).unwrap();
        // config.add(
        //     BgpSession { source: t2, target: b2, session_type: IBgpPeer }
        // ).unwrap();
        config.add(
            BgpSession { source: r1, target: b1, session_type: IBgpClient }
        ).unwrap(); 
        config.add(
            BgpSession { source: r2, target: b1, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: r2, target: b2, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: t1, target: r1, session_type: IBgpClient }
        ).unwrap();
        config.add(
            BgpSession { source: t2, target: r2, session_type: IBgpClient }
        ).unwrap();

        config
    }
    fn get_policy(net: &Network, _variant: usize) -> HardPolicy {
        HardPolicy::reachability(net.get_routers().iter(), net.get_known_prefixes().iter())
    }
}