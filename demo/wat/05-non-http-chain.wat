;; 05-non-http-chain.wat
;;
;; A messaging pipeline demonstrating that cviz detects chains on any
;; interface, not just wasi:http/handler.
;;
;; Topology (request-flow order):
;;
;;   host(wasi:messaging/consumer@0.2.0)
;;     └─→ $consumer         (innermost: imports from host)
;;           └─→ $filter     (applies filtering logic)
;;                 └─→ export(wasi:messaging/consumer@0.2.0)
;;
;; Try it with:
;;   cviz 05-non-http-chain.wasm
;;   cviz -l all-interfaces 05-non-http-chain.wasm
;;   cviz -f mermaid 05-non-http-chain.wasm
(component
    (import "wasi:messaging/consumer@0.2.0" (instance $msg-host
        (export "consume" (func (param "topic" string) (result string)))
    ))

    (component $consumer-backend
        (import "wasi:messaging/consumer@0.2.0" (instance $downstream
            (export "consume" (func (param "topic" string) (result string)))
        ))
        (alias export $downstream "consume" (func $f))
        (instance $out (export "consume" (func $f)))
        (export "wasi:messaging/consumer@0.2.0" (instance $out))
    )

    (instance $consumer (instantiate $consumer-backend
        (with "wasi:messaging/consumer@0.2.0" (instance $msg-host))
    ))
    (alias export $consumer "wasi:messaging/consumer@0.2.0" (instance $consumer-out))

    (component $filter-middleware
        (import "wasi:messaging/consumer@0.2.0" (instance $downstream
            (export "consume" (func (param "topic" string) (result string)))
        ))
        (alias export $downstream "consume" (func $f))
        (instance $out (export "consume" (func $f)))
        (export "wasi:messaging/consumer@0.2.0" (instance $out))
    )

    (instance $filter (instantiate $filter-middleware
        (with "wasi:messaging/consumer@0.2.0" (instance $consumer-out))
    ))
    (alias export $filter "wasi:messaging/consumer@0.2.0" (instance $filter-out))

    (export "wasi:messaging/consumer@0.2.0" (instance $filter-out))
)
