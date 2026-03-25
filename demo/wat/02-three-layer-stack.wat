;; 02-three-layer-stack.wat
;;
;; A three-component HTTP handler chain demonstrating chain depth and ordering.
;;
;; Topology (request-flow order):
;;
;;   host(wasi:http/handler)
;;     └─→ $core              (innermost: imports from host)
;;           └─→ $auth        (middle: adds auth)
;;                 └─→ $rate  (outermost: adds rate-limiting)
;;                       └─→ export(wasi:http/handler)
;;
;; The handler-chain view shows nodes in request-flow order: outermost first.
;; Try it with:
;;   cviz 02-three-layer-stack.wasm
;;   cviz -l all-interfaces 02-three-layer-stack.wasm
;;   cviz -l full 02-three-layer-stack.wasm
(component
    (import "wasi:http/handler@0.3.0" (instance $host
        (export "handle" (func))
    ))

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

    (component $rate-limit-middleware
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func))
        ))
        (alias export $downstream "handle" (func $f))
        (instance $out (export "handle" (func $f)))
        (export "wasi:http/handler@0.3.0" (instance $out))
    )

    (instance $rate (instantiate $rate-limit-middleware
        (with "wasi:http/handler@0.3.0" (instance $auth-out))
    ))
    (alias export $rate "wasi:http/handler@0.3.0" (instance $rate-out))

    (export "wasi:http/handler@0.3.0" (instance $rate-out))
)
