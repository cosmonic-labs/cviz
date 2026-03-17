/// Shared graph builders for unit tests across output modules.
///
/// Each builder constructs a [`CompositionGraph`] representing a specific
/// topology.  Instance indices are assigned as follows so tests can refer
/// to them by number:
///
///   0  — the implicit "host" (any import whose source_instance is 0 and
///         has no corresponding graph node is later marked as a host import
///         by the parser's postprocess step; in unit tests we set
///         `is_host_import` directly instead).
///
/// All builders use consecutive indices starting at 1 for real components.
use crate::model::{
    ComponentNode, CompositionGraph, FuncSignature, InstanceInterface, InterfaceConnection,
    InterfaceType, ValueType,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Simple chain: host → $srv → $middleware → export(wasi:http/handler)
//
//   idx 1  $srv       — imports handler from host
//   idx 2  $middleware — imports handler from $srv, imports log from host
//   export wasi:http/handler@0.3.0 from idx 2
// ---------------------------------------------------------------------------
pub(crate) fn simple_chain_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
    srv.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(1, srv);

    let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
    mw.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    mw.add_import(InterfaceConnection {
        interface_name: "wasi:logging/log@0.1.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(2, mw);

    graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);
    graph
}

// ---------------------------------------------------------------------------
// Two independent chains:
//
//   idx 1  $srv-http  — imports wasi:http/handler from host
//   idx 2  $mw-http   — imports wasi:http/handler from $srv-http
//   export wasi:http/handler@0.3.0 from idx 2
//
//   idx 3  $db        — imports wasi:keyvalue/store from host
//   idx 4  $cache     — imports wasi:keyvalue/store from $db
//   export wasi:keyvalue/store@0.1.0 from idx 4
// ---------------------------------------------------------------------------
pub(crate) fn two_chain_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    // — HTTP chain —
    let mut srv_http = ComponentNode::new("$srv-http".to_string(), 0, 0);
    srv_http.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(1, srv_http);

    let mut mw_http = ComponentNode::new("$mw-http".to_string(), 1, 1);
    mw_http.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(2, mw_http);

    graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);

    // — Keyvalue chain —
    let mut db = ComponentNode::new("$db".to_string(), 2, 2);
    db.add_import(InterfaceConnection {
        interface_name: "wasi:keyvalue/store@0.1.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(3, db);

    let mut cache = ComponentNode::new("$cache".to_string(), 3, 3);
    cache.add_import(InterfaceConnection {
        interface_name: "wasi:keyvalue/store@0.1.0".to_string(),
        source_instance: 3,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(4, cache);

    graph.add_export("wasi:keyvalue/store@0.1.0".to_string(), 4, None);

    graph
}

// ---------------------------------------------------------------------------
// Three-node chain using a non-http interface (wasi:messaging/consumer) to
// demonstrate that chain detection is generic.
//
//   idx 1  $backend  — imports wasi:messaging/consumer from host
//   idx 2  $service  — imports wasi:messaging/consumer from $backend
//   idx 3  $gateway  — imports wasi:messaging/consumer from $service
//   export wasi:messaging/consumer@0.2.0 from idx 3
//
// Request-flow order: $gateway → $service → $backend
// ---------------------------------------------------------------------------
pub(crate) fn long_chain_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    let mut backend = ComponentNode::new("$backend".to_string(), 0, 0);
    backend.add_import(InterfaceConnection {
        interface_name: "wasi:messaging/consumer@0.2.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(1, backend);

    let mut service = ComponentNode::new("$service".to_string(), 1, 1);
    service.add_import(InterfaceConnection {
        interface_name: "wasi:messaging/consumer@0.2.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(2, service);

    let mut gateway = ComponentNode::new("$gateway".to_string(), 2, 2);
    gateway.add_import(InterfaceConnection {
        interface_name: "wasi:messaging/consumer@0.2.0".to_string(),
        source_instance: 2,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(3, gateway);

    graph.add_export("wasi:messaging/consumer@0.2.0".to_string(), 3, None);
    graph
}

// ---------------------------------------------------------------------------
// Chain plus a utility node that isn't part of any chain:
//
//   idx 1  $srv         — imports wasi:http/handler from host
//   idx 2  $middleware  — imports wasi:http/handler from $srv
//   idx 3  $logger      — imports wasi:logging/log from host only
//   export wasi:http/handler@0.3.0 from idx 2
//
// $logger has no inter-component connections so it does NOT form a chain.
// It should appear in AllInterfaces/Full but NOT in HandlerChain.
// ---------------------------------------------------------------------------
pub(crate) fn chain_plus_utility_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
    srv.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(1, srv);

    let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
    mw.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(2, mw);

    let mut logger = ComponentNode::new("$logger".to_string(), 2, 2);
    logger.add_import(InterfaceConnection {
        interface_name: "wasi:logging/log@0.1.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: None,
        fingerprint: None,
    });
    graph.add_node(3, logger);

    graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, None);
    graph
}

// ---------------------------------------------------------------------------
// Type-annotated simple chain (same topology as simple_chain_graph but with
// type info on all connections for type-display tests).
//
// Both connections carry `handle(u32) -> bool`.
// ---------------------------------------------------------------------------
pub(crate) fn typed_chain_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    let u32_id = graph.arena.intern_val(ValueType::U32);
    let bool_id = graph.arena.intern_val(ValueType::Bool);

    let handle_sig = FuncSignature {
        params: vec![u32_id],
        results: vec![bool_id],
    };
    let mut fns = BTreeMap::new();
    fns.insert("handle".to_string(), handle_sig);
    let iface_type = InterfaceType::Instance(InstanceInterface { functions: fns });

    let mut srv = ComponentNode::new("$srv".to_string(), 0, 0);
    srv.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: Some(iface_type.clone()),
        fingerprint: Some(iface_type.fingerprint(&graph.arena)),
    });
    graph.add_node(1, srv);

    let mut mw = ComponentNode::new("$middleware".to_string(), 1, 1);
    mw.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: Some(iface_type.clone()),
        fingerprint: Some(iface_type.fingerprint(&graph.arena)),
    });
    graph.add_node(2, mw);

    graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, Some(iface_type));
    graph
}

// ---------------------------------------------------------------------------
// Two distinct typed chains — handler uses handle(u32)->bool,
// keyvalue uses get(string)->string.  Tests that different types get
// different symbols in the SymbolMap.
// ---------------------------------------------------------------------------
pub(crate) fn two_typed_chain_graph() -> CompositionGraph {
    let mut graph = CompositionGraph::new();

    // handler type: handle(u32) -> bool
    let u32_id = graph.arena.intern_val(ValueType::U32);
    let bool_id = graph.arena.intern_val(ValueType::Bool);
    let handler_sig = FuncSignature {
        params: vec![u32_id],
        results: vec![bool_id],
    };
    let mut handler_fns = BTreeMap::new();
    handler_fns.insert("handle".to_string(), handler_sig);
    let handler_type = InterfaceType::Instance(InstanceInterface {
        functions: handler_fns,
    });

    // keyvalue type: get(string) -> string
    let str_id = graph.arena.intern_val(ValueType::String);
    let get_sig = FuncSignature {
        params: vec![str_id],
        results: vec![str_id],
    };
    let mut kv_fns = BTreeMap::new();
    kv_fns.insert("get".to_string(), get_sig);
    let kv_type = InterfaceType::Instance(InstanceInterface { functions: kv_fns });

    // HTTP chain
    let mut srv_http = ComponentNode::new("$srv-http".to_string(), 0, 0);
    srv_http.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: Some(handler_type.clone()),
        fingerprint: Some(handler_type.fingerprint(&graph.arena)),
    });
    graph.add_node(1, srv_http);

    let mut mw_http = ComponentNode::new("$mw-http".to_string(), 1, 1);
    mw_http.add_import(InterfaceConnection {
        interface_name: "wasi:http/handler@0.3.0".to_string(),
        source_instance: 1,
        is_host_import: false,
        interface_type: Some(handler_type.clone()),
        fingerprint: Some(handler_type.fingerprint(&graph.arena)),
    });
    graph.add_node(2, mw_http);

    graph.add_export("wasi:http/handler@0.3.0".to_string(), 2, Some(handler_type));

    // Keyvalue chain
    let mut db = ComponentNode::new("$db".to_string(), 2, 2);
    db.add_import(InterfaceConnection {
        interface_name: "wasi:keyvalue/store@0.1.0".to_string(),
        source_instance: 0,
        is_host_import: true,
        interface_type: Some(kv_type.clone()),
        fingerprint: Some(kv_type.fingerprint(&graph.arena)),
    });
    graph.add_node(3, db);

    let mut cache = ComponentNode::new("$cache".to_string(), 3, 3);
    cache.add_import(InterfaceConnection {
        interface_name: "wasi:keyvalue/store@0.1.0".to_string(),
        source_instance: 3,
        is_host_import: false,
        interface_type: Some(kv_type.clone()),
        fingerprint: Some(kv_type.fingerprint(&graph.arena)),
    });
    graph.add_node(4, cache);

    graph.add_export("wasi:keyvalue/store@0.1.0".to_string(), 4, Some(kv_type));

    graph
}
