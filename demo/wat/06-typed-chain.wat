;; 06-typed-chain.wat
;;
;; Same topology as 01-simple-chain but with a richer WIT-style interface:
;; the handler function takes two parameters and returns two values, so that
;; `cviz --types true` produces a meaningful type annotation key.
;;
;; Topology (request-flow order):
;;
;;   host(wasi:http/handler@0.3.0)
;;     └─→ $core        (innermost: imports from host)
;;           └─→ $auth  (outermost: adds auth logic)
;;                 └─→ export(wasi:http/handler@0.3.0)
;;
;; The handler interface carries:
;;   handle(method: string, path: string) -> u32
;;
;; Try it with:
;;   cviz 06-typed-chain.wasm                   (types on by default)
;;   cviz --types false 06-typed-chain.wasm     (types off — no key section)
;;   cviz -f mermaid 06-typed-chain.wasm
(component
    (import "wasi:http/handler@0.3.0" (instance $host
        (export "handle" (func (param "method" string) (param "path" string) (result u32)))
    ))

    (component $core-handler
        (import "wasi:http/handler@0.3.0" (instance $downstream
            (export "handle" (func (param "method" string) (param "path" string) (result u32)))
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
            (export "handle" (func (param "method" string) (param "path" string) (result u32)))
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
