;; 04-chain-plus-utility.wat
;;
;; An HTTP handler chain plus a logging utility component that is NOT part of
;; any chain.
;;
;; Topology:
;;
;;   HTTP handler chain:
;;     host(wasi:http/handler) → $core → $auth → export(wasi:http/handler)
;;
;;   Logging utility (sidecar):
;;     host(wasi:logging/log) → $logger
;;     $logger is wired to $auth so the middleware can log, but
;;     wasi:logging/log is never exported from the composition — so $logger
;;     forms no chain.
;;
;; This demonstrates the difference between HandlerChain and AllInterfaces:
;;   HandlerChain shows only $core and $auth (the handler chain).
;;   AllInterfaces also shows $logger connected to the host via a dashed edge.
;;
;; Try it with:
;;   cviz 04-chain-plus-utility.wasm
;;   cviz -l all-interfaces 04-chain-plus-utility.wasm
;;   cviz -f mermaid 04-chain-plus-utility.wasm
(component
    (import "wasi:http/handler@0.3.0" (instance $http-host
        (export "handle" (func))
    ))
    (import "wasi:logging/log@0.1.0" (instance $log-host
        (export "log" (func (param "level" u32) (param "message" string)))
    ))

    ;; — Logging utility (not part of any chain) —

    (component $logging-provider
        (import "wasi:logging/log@0.1.0" (instance $log
            (export "log" (func (param "level" u32) (param "message" string)))
        ))
        (alias export $log "log" (func $log-fn))
        (instance $out (export "log" (func $log-fn)))
        (export "wasi:logging/log@0.1.0" (instance $out))
    )

    (instance $logger (instantiate $logging-provider
        (with "wasi:logging/log@0.1.0" (instance $log-host))
    ))
    (alias export $logger "wasi:logging/log@0.1.0" (instance $logger-out))

    ;; — HTTP handler chain —

    (component $core-handler
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (import "wasi:logging/log@0.1.0" (instance $log
            (export "log" (func (param "level" u32) (param "message" string)))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $core (instantiate $core-handler
        (with "wasi:http/handler@0.3.0" (instance $http-host))
        (with "wasi:logging/log@0.1.0" (instance $logger-out))
    ))
    (alias export $core "wasi:http/handler@0.3.0" (instance $core-out))

    (component $auth-middleware
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $auth (instantiate $auth-middleware
        (with "wasi:http/handler@0.3.0" (instance $core-out))
    ))
    (alias export $auth "wasi:http/handler@0.3.0" (instance $auth-out))

    ;; Export handler only — logging is internal, so $logger is NOT a chain
    (export "wasi:http/handler@0.3.0" (instance $auth-out))
)
