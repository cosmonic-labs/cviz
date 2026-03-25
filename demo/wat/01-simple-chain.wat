;; 01-simple-chain.wat
;;
;; A basic two-component HTTP handler chain.
;;
;; Topology (request-flow order):
;;
;;   host(wasi:http/handler)
;;     └─→ $core          (innermost: imports handler from host)
;;           └─→ $auth    (outermost: imports handler from $core)
;;                 └─→ export(wasi:http/handler)
;;
;; This is the simplest possible middleware chain.  Try it with:
;;   cviz 01-simple-chain.wasm
;;   cviz -l all-interfaces 01-simple-chain.wasm
;;   cviz -f mermaid 01-simple-chain.wasm
(component
    (import "wasi:http/handler@0.3.0" (instance $host
        (export "handle" (func))
    ))

    ;; Innermost component: handles requests using the host handler
    (component $core-handler
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $core (instantiate $core-handler
        (with "wasi:http/handler@0.3.0" (instance $host))
    ))
    (alias export $core "wasi:http/handler@0.3.0" (instance $core-out))

    ;; Outermost component: wraps $core, adds auth logic
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

    (export "wasi:http/handler@0.3.0" (instance $auth-out))
)
