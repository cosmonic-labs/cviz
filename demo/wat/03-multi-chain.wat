;; 03-multi-chain.wat
;;
;; Two independent chains in a single composition, demonstrating that cviz's
;; chain detection is generic and not limited to HTTP handler.
;;
;; Topology:
;;
;;   HTTP handler chain:
;;     host(wasi:http/handler) → $http-core → $http-auth → export(wasi:http/handler)
;;
;;   Keyvalue chain:
;;     host(wasi:keyvalue/store) → $kv-store → $kv-cache → export(wasi:keyvalue/store)
;;
;; The handler-chain view renders both chains separately.  Try it with:
;;   cviz 03-multi-chain.wasm
;;   cviz -l all-interfaces 03-multi-chain.wasm
;;   cviz -f mermaid 03-multi-chain.wasm
;;   cviz -f mermaid --direction td 03-multi-chain.wasm
(component
    (import "wasi:http/handler@0.3.0" (instance $http-host
        (export "handle" (func))
    ))
    (import "wasi:keyvalue/store@0.1.0" (instance $kv-host
        (export "get" (func (param "key" string) (result string)))
    ))

    ;; — HTTP handler chain —

    (component $http-core-handler
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $http-core (instantiate $http-core-handler
        (with "wasi:http/handler@0.3.0" (instance $http-host))
    ))
    (alias export $http-core "wasi:http/handler@0.3.0" (instance $http-core-out))

    (component $http-auth-middleware
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $http-auth (instantiate $http-auth-middleware
        (with "wasi:http/handler@0.3.0" (instance $http-core-out))
    ))
    (alias export $http-auth "wasi:http/handler@0.3.0" (instance $http-auth-out))

    ;; — Keyvalue chain —

    (component $kv-store-backend
        (import "wasi:keyvalue/store@0.1.0" (instance $downstream
            (export "get" (func (param "key" string) (result string)))
        ))
        (alias export $downstream "get" (func $g))
        (instance $out (export "get" (func $g)))
        (export "wasi:keyvalue/store@0.1.0" (instance $out))
    )

    (instance $kv-store (instantiate $kv-store-backend
        (with "wasi:keyvalue/store@0.1.0" (instance $kv-host))
    ))
    (alias export $kv-store "wasi:keyvalue/store@0.1.0" (instance $kv-store-out))

    (component $kv-cache-middleware
        (import "wasi:keyvalue/store@0.1.0" (instance $downstream
            (export "get" (func (param "key" string) (result string)))
        ))
        (alias export $downstream "get" (func $g))
        (instance $out (export "get" (func $g)))
        (export "wasi:keyvalue/store@0.1.0" (instance $out))
    )

    (instance $kv-cache (instantiate $kv-cache-middleware
        (with "wasi:keyvalue/store@0.1.0" (instance $kv-store-out))
    ))
    (alias export $kv-cache "wasi:keyvalue/store@0.1.0" (instance $kv-cache-out))

    ;; Exports
    (export "wasi:http/handler@0.3.0" (instance $http-auth-out))
    (export "wasi:keyvalue/store@0.1.0" (instance $kv-cache-out))
)
