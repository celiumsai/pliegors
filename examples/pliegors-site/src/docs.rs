use crate::components::{brand_mark, locale_path};
use crate::content::Locale;
use pliego_dom::{IntoView, View, el};

#[derive(Clone, Copy)]
pub struct DocTopic {
    pub slug: &'static str,
    pub group_en: &'static str,
    pub group_es: &'static str,
    pub title_en: &'static str,
    pub title_es: &'static str,
    pub summary_en: &'static str,
    pub summary_es: &'static str,
}

pub const TOPICS: &[DocTopic] = &[
    DocTopic {
        slug: "getting-started",
        group_en: "Start",
        group_es: "Inicio",
        title_en: "Getting started",
        title_es: "Primeros pasos",
        summary_en: "Install the CLI, create a project, start the development server, and produce the first verified build.",
        summary_es: "Instala el CLI, crea un proyecto, inicia el servidor de desarrollo y produce el primer build verificado.",
    },
    DocTopic {
        slug: "project-structure",
        group_en: "Start",
        group_es: "Inicio",
        title_en: "Project structure",
        title_es: "Estructura del proyecto",
        summary_en: "Understand pliego.toml, the site package, the optional WASM client, public assets, and output ownership.",
        summary_es: "Entiende pliego.toml, el paquete del sitio, el cliente WASM opcional, los assets públicos y la propiedad de la salida.",
    },
    DocTopic {
        slug: "cli",
        group_en: "Start",
        group_es: "Inicio",
        title_en: "CLI reference",
        title_es: "Referencia del CLI",
        summary_en: "Use new, check, build, dev, preview, inspect, templates, and machine-readable diagnostics.",
        summary_es: "Usa new, check, build, dev, preview, inspect, templates y diagnósticos legibles por máquinas.",
    },
    DocTopic {
        slug: "routing-and-pages",
        group_en: "Author",
        group_es: "Autoría",
        title_en: "Routing and pages",
        title_es: "Routing y páginas",
        summary_en: "Create complete HTML documents, canonical metadata, clean routes, redirects, and authored error pages.",
        summary_es: "Crea documentos HTML completos, metadata canónica, rutas limpias, redirects y páginas de error con autoría.",
    },
    DocTopic {
        slug: "views",
        group_en: "Author",
        group_es: "Autoría",
        title_en: "Rust views",
        title_es: "Vistas Rust",
        summary_en: "Compose semantic views with escaped text, typed macros, stable attributes, and useful HTML first.",
        summary_es: "Compón vistas semánticas con texto escapado, macros tipadas, atributos estables y HTML útil primero.",
    },
    DocTopic {
        slug: "events-and-folds",
        group_en: "State",
        group_es: "Estado",
        title_en: "Events and folds",
        title_es: "Eventos y folds",
        summary_en: "Model durable facts, deterministic projections, replay parity, snapshots, and explicit effects.",
        summary_es: "Modela hechos durables, proyecciones deterministas, paridad de replay, snapshots y efectos explícitos.",
    },
    DocTopic {
        slug: "content",
        group_en: "Author",
        group_es: "Autoría",
        title_en: "Typed content",
        title_es: "Contenido tipado",
        summary_en: "Load bounded Markdown, JSON, and TOML collections with stable identities and actionable errors.",
        summary_es: "Carga colecciones limitadas de Markdown, JSON y TOML con identidades estables y errores accionables.",
    },
    DocTopic {
        slug: "browser-runtime",
        group_en: "Runtime",
        group_es: "Runtime",
        title_en: "Browser runtime",
        title_es: "Runtime del navegador",
        summary_en: "Resume Rust/WASM behavior and admit GSAP, Lenis, Three.js, or WebGL through lifecycle adapters.",
        summary_es: "Reanuda comportamiento Rust/WASM y admite GSAP, Lenis, Three.js o WebGL mediante adaptadores de lifecycle.",
    },
    DocTopic {
        slug: "assets",
        group_en: "Delivery",
        group_es: "Entrega",
        title_en: "Adaptive assets",
        title_es: "Assets adaptativos",
        summary_en: "Plan reproducible images, video, fonts, and 3D variants under explicit device budgets.",
        summary_es: "Planifica variantes reproducibles de imagen, video, fuentes y 3D bajo presupuestos explícitos por dispositivo.",
    },
    DocTopic {
        slug: "errors-and-diagnostics",
        group_en: "Operate",
        group_es: "Operación",
        title_en: "Errors and diagnostics",
        title_es: "Errores y diagnósticos",
        summary_en: "Read stable PLG codes, browser build failures, JSON diagnostics, exit codes, and recovery actions.",
        summary_es: "Interpreta códigos PLG estables, fallos de build en navegador, diagnósticos JSON, códigos de salida y recuperación.",
    },
    DocTopic {
        slug: "build-and-deploy",
        group_en: "Delivery",
        group_es: "Entrega",
        title_en: "Build and deploy",
        title_es: "Build y despliegue",
        summary_en: "Verify the output ledger, preview production bytes, select release artifacts, and deploy static output.",
        summary_es: "Verifica el ledger de salida, previsualiza bytes de producción, selecciona artefactos y despliega salida estática.",
    },
    DocTopic {
        slug: "licensing",
        group_en: "Project",
        group_es: "Proyecto",
        title_en: "Licensing and policy",
        title_es: "Licenciamiento y políticas",
        summary_en: "Understand Apache-2.0, third-party notices, trademarks, security reports, support, and contributions.",
        summary_es: "Entiende Apache-2.0, avisos de terceros, marcas, reportes de seguridad, soporte y contribuciones.",
    },
];

pub fn index(locale: Locale) -> View {
    let mut topics = el("div").class("rs-doc-grid");
    for (index, topic) in TOPICS.iter().enumerate() {
        let title = localized(locale, topic.title_en, topic.title_es);
        let summary = localized(locale, topic.summary_en, topic.summary_es);
        topics = topics.child(
            el("a")
                .class("rs-doc-card")
                .attr(
                    "href",
                    locale_path(locale, &format!("/docs/{}", topic.slug)),
                )
                .attr("data-docs-item", "")
                .attr("data-search", format!("{title} {summary}").to_lowercase())
                .child(el("span").child(format!("{:02}", index + 1)))
                .child(el("p").class("utility-label").child(localized(
                    locale,
                    topic.group_en,
                    topic.group_es,
                )))
                .child(el("h2").child(title))
                .child(el("p").child(summary))
                .child(el("b").attr("aria-hidden", "true").child("↗")),
        );
    }

    el("section")
        .class("rs-docs")
        .attr("data-docs-page", "")
        .child(docs_hero(locale))
        .child(
            el("section")
                .class("rs-doc-start")
                .attr("aria-labelledby", "docs-start-title")
                .child(
                    el("div")
                        .child(el("p").class("utility-label").child("START / 04 COMMANDS"))
                        .child(el("h2").id("docs-start-title").child(localized(
                            locale,
                            "From zero to a running project.",
                            "De cero a un proyecto en ejecución.",
                        )))
                        .child(el("p").child(localized(
                            locale,
                            "The default starter is intentionally small, but it is not blank: it teaches the route graph, authored errors, metadata, assets, and the build ledger.",
                            "El starter predeterminado es intencionalmente pequeño, pero no está vacío: enseña el grafo de rutas, errores con autoría, metadata, assets y el ledger del build.",
                        ))),
                )
                .child(code_block(
                    locale,
                    "shell",
                    "pliego new my-app\ncd my-app\npliego check\npliego dev",
                )),
        )
        .child(
            el("div")
                .class("rs-doc-search")
                .child(
                    el("label")
                        .attr("for", "docs-search")
                        .child(localized(locale, "Search documentation", "Buscar documentación")),
                )
                .child(
                    el("input")
                        .id("docs-search")
                        .attr("type", "search")
                        .attr("data-docs-search", "")
                        .attr(
                            "placeholder",
                            localized(locale, "Type a concept…", "Escribe un concepto…"),
                        ),
                )
                .child(
                    el("button")
                        .attr("type", "button")
                        .attr("data-docs-clear", "")
                        .attr(
                            "aria-label",
                            localized(locale, "Clear search", "Limpiar búsqueda"),
                        )
                        .child("×"),
                ),
        )
        .child(topics)
        .into_view()
}

pub fn article(locale: Locale, slug: &str) -> Result<View, String> {
    let topic_index = TOPICS
        .iter()
        .position(|topic| topic.slug == slug)
        .ok_or_else(|| format!("unknown documentation topic {slug}"))?;
    let topic = TOPICS[topic_index];
    let outline = outline(slug);
    let content = match slug {
        "getting-started" => getting_started(locale),
        "project-structure" => project_structure(locale),
        "cli" => cli_reference(locale),
        "routing-and-pages" => routing_and_pages(locale),
        "views" => views(locale),
        "events-and-folds" => events_and_folds(locale),
        "content" => typed_content(locale),
        "browser-runtime" => browser_runtime(locale),
        "assets" => adaptive_assets(locale),
        "errors-and-diagnostics" => errors_and_diagnostics(locale),
        "build-and-deploy" => build_and_deploy(locale),
        "licensing" => licensing(locale),
        _ => unreachable!("topic registry and renderer stay aligned"),
    };

    Ok(el("section")
        .class("rs-doc-article")
        .child(article_hero(locale, topic, topic_index + 1))
        .child(mobile_topic_navigation(locale, slug))
        .child(
            el("div")
                .class("rs-doc-layout")
                .child(topic_navigation(locale, slug))
                .child(el("article").class("rs-doc-content").child(content))
                .child(on_this_page(locale, &outline)),
        )
        .child(pagination(locale, topic_index))
        .into_view())
}

fn docs_hero(locale: Locale) -> View {
    el("header")
        .class("rs-docs-hero")
        .child(
            el("div")
                .class("rs-docs-hero__mark")
                .attr("aria-hidden", "true")
                .child(brand_mark(72, None)),
        )
        .child(el("p").class("utility-label").child("PLIEGORS / DOCUMENTATION"))
        .child(el("h1").child(localized(
            locale,
            "Build the whole document in Rust.",
            "Construye todo el documento en Rust.",
        )))
        .child(el("p").child(localized(
            locale,
            "Start with useful HTML, add state that can explain itself, and admit browser libraries only through explicit lifecycle boundaries.",
            "Comienza con HTML útil, añade estado capaz de explicarse y admite librerías del navegador sólo mediante límites de lifecycle explícitos.",
        )))
        .into_view()
}

fn article_hero(locale: Locale, topic: DocTopic, index: usize) -> View {
    el("header")
        .class("rs-doc-article__hero")
        .child(
            el("a")
                .class("rs-doc-breadcrumb")
                .attr("href", locale_path(locale, "/docs"))
                .child(localized(locale, "Documentation", "Documentación"))
                .child(el("span").attr("aria-hidden", "true").child("/")),
        )
        .child(el("p").class("utility-label").child(format!(
            "DOC / {:02} / {}",
            index,
            localized(locale, topic.group_en, topic.group_es).to_uppercase()
        )))
        .child(el("h1").child(localized(locale, topic.title_en, topic.title_es)))
        .child(el("p").class("rs-doc-article__lead").child(localized(
            locale,
            topic.summary_en,
            topic.summary_es,
        )))
        .into_view()
}

fn topic_navigation(locale: Locale, active_slug: &str) -> View {
    let mut nav = el("nav")
        .class("rs-doc-sidebar")
        .attr(
            "aria-label",
            localized(
                locale,
                "Documentation sections",
                "Secciones de documentación",
            ),
        )
        .child(
            el("a")
                .class("rs-doc-sidebar__index")
                .attr("href", locale_path(locale, "/docs"))
                .child(localized(
                    locale,
                    "Documentation index",
                    "Índice de documentación",
                )),
        );
    for (index, topic) in TOPICS.iter().enumerate() {
        let mut link = el("a")
            .attr(
                "href",
                locale_path(locale, &format!("/docs/{}", topic.slug)),
            )
            .child(el("span").child(format!("{:02}", index + 1)))
            .child(localized(locale, topic.title_en, topic.title_es));
        if topic.slug == active_slug {
            link = link.attr("aria-current", "page");
        }
        nav = nav.child(link);
    }
    nav.into_view()
}

fn mobile_topic_navigation(locale: Locale, active_slug: &str) -> View {
    let active = TOPICS
        .iter()
        .find(|topic| topic.slug == active_slug)
        .expect("active topic");
    let mut list = el("div");
    for topic in TOPICS {
        list = list.child(
            el("a")
                .attr(
                    "href",
                    locale_path(locale, &format!("/docs/{}", topic.slug)),
                )
                .attr(
                    "aria-current",
                    if topic.slug == active_slug {
                        "page"
                    } else {
                        "false"
                    },
                )
                .child(localized(locale, topic.title_en, topic.title_es)),
        );
    }
    el("details")
        .class("rs-doc-mobile-nav")
        .child(
            el("summary")
                .child(localized(locale, "In this guide", "En esta guía"))
                .child(el("b").child(localized(locale, active.title_en, active.title_es))),
        )
        .child(list)
        .into_view()
}

fn on_this_page(locale: Locale, outline: &[(&str, &str, &str)]) -> View {
    let mut nav = el("nav")
        .class("rs-doc-outline")
        .attr(
            "aria-label",
            localized(locale, "On this page", "En esta página"),
        )
        .child(el("p").class("utility-label").child(localized(
            locale,
            "On this page",
            "En esta página",
        )));
    for (id, en, es) in outline {
        nav = nav.child(
            el("a")
                .attr("href", format!("#{id}"))
                .child(localized(locale, en, es)),
        );
    }
    nav.into_view()
}

fn pagination(locale: Locale, index: usize) -> View {
    let mut nav = el("nav").class("rs-doc-pagination").attr(
        "aria-label",
        localized(
            locale,
            "Documentation pagination",
            "Paginación de documentación",
        ),
    );
    if let Some(previous) = index.checked_sub(1).and_then(|item| TOPICS.get(item)) {
        nav = nav.child(pagination_link(
            locale, previous, "←", "PREVIOUS", "ANTERIOR",
        ));
    } else {
        nav = nav.child(el("span"));
    }
    if let Some(next) = TOPICS.get(index + 1) {
        nav = nav.child(pagination_link(locale, next, "→", "NEXT", "SIGUIENTE"));
    }
    nav.into_view()
}

fn pagination_link(locale: Locale, topic: &DocTopic, arrow: &str, en: &str, es: &str) -> View {
    el("a")
        .attr(
            "href",
            locale_path(locale, &format!("/docs/{}", topic.slug)),
        )
        .child(el("span").child(localized(locale, en, es)))
        .child(el("strong").child(localized(locale, topic.title_en, topic.title_es)))
        .child(el("b").attr("aria-hidden", "true").child(arrow.to_owned()))
        .into_view()
}

fn getting_started(locale: Locale) -> View {
    vec![
        doc_section(
            locale,
            "requirements",
            "Before you begin",
            "Antes de comenzar",
            vec![
                paragraph(locale, "PliegoRS projects are Rust workspaces. Install a stable Rust toolchain, the wasm32-unknown-unknown target when the project has a browser client, and wasm-bindgen-cli at the exact version reported by pliego check.", "Los proyectos PliegoRS son workspaces Rust. Instala un toolchain Rust estable, el target wasm32-unknown-unknown cuando el proyecto tenga cliente de navegador y wasm-bindgen-cli en la versión exacta indicada por pliego check."),
                definition_list(locale, &[
                    ("Rust", "1.85 or the release toolchain declared by the project", "1.85 o el toolchain de release declarado por el proyecto"),
                    ("Targets", "Linux x64/ARM64 for production; macOS and Windows for development", "Linux x64/ARM64 para producción; macOS y Windows para desarrollo"),
                    ("Source", "GitHub Releases and the canonical celiumsai/pliegors repository", "GitHub Releases y el repositorio canónico celiumsai/pliegors"),
                ]),
            ],
        ),
        doc_section(
            locale,
            "install",
            "Install the CLI",
            "Instalar el CLI",
            vec![
                note(locale, "Release gate", "Public installers become available with the first non-draft GitHub Release. Until then, authorized contributors build the CLI from the repository with --locked.", "Gate de release", "Los instaladores públicos estarán disponibles con el primer GitHub Release no draft. Hasta entonces, los contribuidores autorizados compilan el CLI desde el repositorio con --locked."),
                code_block(locale, "shell", "git clone https://github.com/celiumsai/pliegors.git\ncd pliegors\ncargo build --locked --release -p pliego-cli\n./target/release/pliego version"),
                paragraph(locale, "Release installers are downloaded to disk, verified, and then executed. PliegoRS never documents piping an unverified network response directly into a shell.", "Los instaladores de release se descargan a disco, se verifican y después se ejecutan. PliegoRS nunca documenta enviar una respuesta de red sin verificar directamente a un shell."),
            ],
        ),
        doc_section(
            locale,
            "create",
            "Create and run a project",
            "Crear y ejecutar un proyecto",
            vec![
                code_block(locale, "shell", "pliego new my-app\ncd my-app\npliego check\npliego dev"),
                steps(locale, &[
                    ("Scaffold", "pliego new writes the default onboarding project transactionally; it never merges into a non-empty directory.", "Scaffold", "pliego new escribe el proyecto de onboarding predeterminado de forma transaccional; nunca mezcla archivos en un directorio no vacío."),
                    ("Check", "pliego check validates the manifest, Cargo packages, Rust target, and wasm-bindgen contract without producing output.", "Verificar", "pliego check valida el manifest, los paquetes Cargo, el target Rust y el contrato wasm-bindgen sin producir salida."),
                    ("Develop", "pliego dev builds, serves on 127.0.0.1:4400, watches source files, and presents build failures in the browser.", "Desarrollar", "pliego dev compila, sirve en 127.0.0.1:4400, observa los archivos fuente y presenta fallos de build en el navegador."),
                    ("Verify", "pliego build && pliego inspect produces and verifies the production artifact ledger.", "Verificar salida", "pliego build && pliego inspect produce y verifican el ledger del artefacto de producción."),
                ]),
            ],
        ),
        doc_section(
            locale,
            "next",
            "Where to go next",
            "Qué sigue",
            vec![link_list(locale, &[
                ("/docs/project-structure", "Read the project tree", "Lee el árbol del proyecto"),
                ("/docs/routing-and-pages", "Add a route", "Añade una ruta"),
                ("/docs/errors-and-diagnostics", "Understand build failures", "Entiende los fallos de build"),
            ])],
        ),
    ]
    .into_view()
}

fn project_structure(locale: Locale) -> View {
    vec![
        doc_section(locale, "tree", "The default tree", "El árbol predeterminado", vec![
            code_block(locale, "text", "my-app/\n├── Cargo.toml\n├── pliego.toml\n├── README.md\n├── LICENSE\n├── assets/\n│   ├── favicon.svg\n│   ├── site.css\n│   ├── site.webmanifest\n│   └── robots.txt\n└── src/\n    └── main.rs"),
            paragraph(locale, "A site can remain one Rust package or grow into a workspace with a native site package and a separate cdylib browser client. The manifest owns that boundary explicitly.", "Un sitio puede permanecer como un paquete Rust o crecer hacia un workspace con un paquete nativo del sitio y un cliente de navegador cdylib separado. El manifest controla ese límite explícitamente."),
        ]),
        doc_section(locale, "manifest", "pliego.toml", "pliego.toml", vec![
            code_block(locale, "toml", "[project]\nname = \"My App\"\nsite_package = \"my-app\"\noutput = \"target/site\""),
            paragraph(locale, "Generated paths must stay inside the project, use portable names, and avoid the source tree. The nearest pliego.toml defines the active project for every CLI command.", "Las rutas generadas deben permanecer dentro del proyecto, usar nombres portables y evitar el árbol de fuentes. El pliego.toml más cercano define el proyecto activo para cada comando del CLI."),
        ]),
        doc_section(locale, "packages", "Site and client packages", "Paquetes de sitio y cliente", vec![
            definition_list(locale, &[
                ("site_package", "A native binary that authors complete pages and writes the output ledger", "Un binario nativo que crea páginas completas y escribe el ledger de salida"),
                ("client.package", "An optional cdylib compiled to wasm32-unknown-unknown", "Un cdylib opcional compilado a wasm32-unknown-unknown"),
                ("client.bindgen_output", "Generated JS/WASM glue consumed by the site package", "Glue JS/WASM generado y consumido por el paquete del sitio"),
            ]),
        ]),
        doc_section(locale, "output", "Output is an artifact", "La salida es un artefacto", vec![
            paragraph(locale, "target/site is replaced atomically after a successful build. pliego.build.json binds every emitted file by path, size, and SHA-256; preview and inspect reject a missing or invalid ledger.", "target/site se reemplaza atómicamente después de un build exitoso. pliego.build.json vincula cada archivo emitido por ruta, tamaño y SHA-256; preview e inspect rechazan un ledger ausente o inválido."),
            note(locale, "Do not edit target/site", "Changes in the output directory are generated evidence and will be replaced by the next build.", "No edites target/site", "Los cambios en el directorio de salida son evidencia generada y serán reemplazados por el siguiente build."),
        ]),
    ].into_view()
}

fn cli_reference(locale: Locale) -> View {
    vec![
        doc_section(locale, "commands", "Command surface", "Superficie de comandos", vec![
            command_table(locale),
        ]),
        doc_section(locale, "development", "Development servers", "Servidores de desarrollo", vec![
            code_block(locale, "shell", "pliego dev\npliego dev 4300\npliego dev 4300 --lan\npliego preview 4400 --host 127.0.0.1"),
            paragraph(locale, "dev rebuilds and injects a bounded EventSource reload hook that never enters production output. preview serves only an already verified build. Both bind to loopback unless --lan or --host is explicit.", "dev recompila e inyecta un hook EventSource limitado que nunca entra en la salida de producción. preview sirve únicamente un build ya verificado. Ambos se enlazan a loopback salvo que --lan o --host sean explícitos."),
        ]),
        doc_section(locale, "diagnostics", "Machine-readable diagnostics", "Diagnósticos legibles por máquinas", vec![
            code_block(locale, "shell", "pliego build --diagnostic-format json"),
            code_block(locale, "json", "{\n  \"code\": \"PLG-BLD-001\",\n  \"exit_code\": 5,\n  \"category\": \"build\",\n  \"message\": \"…\",\n  \"help\": \"Correct the compiler error and run pliego build again.\"\n}"),
            paragraph(locale, "Human and JSON diagnostics carry the same stable code, category, message, recovery action, and exit status.", "Los diagnósticos humanos y JSON contienen el mismo código estable, categoría, mensaje, acción de recuperación y estado de salida."),
        ]),
        doc_section(locale, "exit-codes", "Exit codes", "Códigos de salida", vec![
            definition_list(locale, &[
                ("2 / PLG-ARG", "Invalid command or option", "Comando u opción inválida"),
                ("3 / PLG-PRJ, PLG-NEW", "Project discovery or scaffold failure", "Fallo de descubrimiento del proyecto o scaffold"),
                ("4 / PLG-ENV", "Toolchain or package contract failure", "Fallo del toolchain o contrato de paquetes"),
                ("5 / PLG-BLD", "Compilation or site build failure", "Fallo de compilación o build del sitio"),
                ("6 / PLG-ART", "Artifact or ledger verification failure", "Fallo de verificación del artefacto o ledger"),
                ("7 / PLG-SRV", "Development or preview server failure", "Fallo del servidor de desarrollo o preview"),
            ]),
        ]),
    ].into_view()
}

fn routing_and_pages(locale: Locale) -> View {
    vec![
        doc_section(locale, "page", "Author a complete page", "Crea una página completa", vec![
            code_block(locale, "rust", "use pliego_dom::{IntoView, el};\nuse pliego_ssg::{Head, Page};\n\nlet page = Page::new(\n    \"/guide\",\n    Head::new(\"Guide | My App\")\n        .description(\"A complete authored page.\")\n        .canonical(\"https://example.com/guide\"),\n    el(\"main\")\n        .child(el(\"h1\").child(\"Guide\"))\n        .into_view(),\n);"),
            paragraph(locale, "A Page owns its route, Head, body, and optional language. PliegoRS emits useful HTML directly; routing does not begin as a client-side application shell.", "Una Page controla su ruta, Head, body y lenguaje opcional. PliegoRS emite HTML útil directamente; el routing no comienza como un shell de aplicación en el cliente."),
        ]),
        doc_section(locale, "routes", "Route normalization", "Normalización de rutas", vec![
            paragraph(locale, "Clean routes publish to index documents. /guide and /guide/ resolve to guide/index.html. Distinct authored routes that normalize to the same portable output are rejected before staging.", "Las rutas limpias se publican como documentos index. /guide y /guide/ resuelven a guide/index.html. Rutas de autoría distintas que normalizan a la misma salida portable se rechazan antes del staging."),
            note(locale, "Portable namespace", "Case-only differences, Windows reserved names, parent traversal, aliases, pages, assets, and the ledger share one collision model.", "Namespace portable", "Diferencias sólo de mayúsculas, nombres reservados de Windows, traversal, aliases, páginas, assets y ledger comparten un modelo de colisiones."),
        ]),
        doc_section(locale, "head", "Metadata and canonical identity", "Metadata e identidad canónica", vec![
            code_block(locale, "rust", "Head::new(\"My page\")\n    .description(\"A precise description.\")\n    .canonical(\"https://example.com/my-page\")\n    .icon(\"/favicon.svg\")\n    .manifest(\"/site.webmanifest\")\n    .meta(\"robots\", \"index, follow\")\n    .property_meta(\"og:type\", \"website\")"),
        ]),
        doc_section(locale, "errors", "Authored error pages", "Páginas de error con autoría", vec![
            paragraph(locale, "Every maintained starter emits /404.html. The preview server returns that document with HTTP 404 for unknown routes. If it is missing, PliegoRS serves a branded fallback that explains how to add one.", "Cada starter mantenido emite /404.html. El servidor preview devuelve ese documento con HTTP 404 para rutas desconocidas. Si falta, PliegoRS sirve un fallback de marca que explica cómo añadirlo."),
            code_block(locale, "rust", "site.page(Page::new(\n    \"/404.html\",\n    error_head(\"Route not found\"),\n    not_found(),\n));"),
        ]),
    ].into_view()
}

fn views(locale: Locale) -> View {
    vec![
        doc_section(locale, "composition", "Compose semantic HTML", "Compón HTML semántico", vec![
            code_block(locale, "rust", "use pliego_dom::{IntoView, View, el};\n\nfn notice(title: &str, body: &str) -> View {\n    el(\"aside\")\n        .attr(\"aria-labelledby\", \"notice-title\")\n        .child(el(\"h2\").id(\"notice-title\").child(title.to_owned()))\n        .child(el(\"p\").child(body.to_owned()))\n        .into_view()\n}"),
            paragraph(locale, "Text nodes and attribute values are escaped by the renderer. Structural names are validated at construction boundaries; raw HTML is never the default path.", "Los nodos de texto y valores de atributos son escapados por el renderer. Los nombres estructurales se validan en los límites de construcción; HTML crudo nunca es la ruta predeterminada."),
        ]),
        doc_section(locale, "macro", "Typed view macro", "Macro de vista tipada", vec![
            code_block(locale, "rust", "use pliego_macros::view;\n\nlet page = view! {\n    <main class=\"shell\">\n        <h1>{title}</h1>\n        <p>{summary}</p>\n    </main>\n};"),
            paragraph(locale, "Use the builder for dynamic composition and view! for concise authored trees. Both produce the same View contract and deterministic HTML.", "Usa el builder para composición dinámica y view! para árboles concisos. Ambos producen el mismo contrato View y HTML determinista."),
        ]),
        doc_section(locale, "static-first", "Useful HTML first", "HTML útil primero", vec![
            paragraph(locale, "Navigation, content, forms, headings, landmarks, and essential controls belong in the authored document. Rust/WASM resumes owned behavior at explicit boundaries instead of reconstructing the page.", "Navegación, contenido, formularios, headings, landmarks y controles esenciales pertenecen al documento creado. Rust/WASM reanuda comportamiento controlado en límites explícitos en vez de reconstruir la página."),
            note(locale, "Accessibility is structural", "Use native elements and complete labels before adding client behavior. Reduced motion must preserve meaning and every action.", "La accesibilidad es estructural", "Usa elementos nativos y labels completos antes de añadir comportamiento en el cliente. El movimiento reducido debe preservar significado y cada acción."),
        ]),
        doc_section(locale, "ownership", "Mounted ownership", "Propiedad montada", vec![
            paragraph(locale, "A mounted scope owns its reactive children, DOM listeners, adapter instances, cancellation handles, and cleanup. Unmounting the scope disposes the complete subtree.", "Un scope montado controla sus hijos reactivos, listeners DOM, instancias de adapters, handles de cancelación y cleanup. Desmontar el scope dispone el subárbol completo."),
        ]),
    ].into_view()
}

fn events_and_folds(locale: Locale) -> View {
    vec![
        doc_section(locale, "model", "Facts, not mutations", "Hechos, no mutaciones", vec![
            paragraph(locale, "Significant state begins as a typed event. A fold projects those facts into the state required by the interface. Replaying the same accepted history must produce the same state and output.", "El estado significativo comienza como un evento tipado. Un fold proyecta esos hechos hacia el estado requerido por la interfaz. Reproducir la misma historia aceptada debe producir el mismo estado y salida."),
            code_block(locale, "rust", "#[derive(Clone)]\nenum TaskEvent {\n    Created { id: u64, title: String },\n    Completed { id: u64 },\n}\n\nfn reduce(state: &mut Tasks, event: &TaskEvent) {\n    match event {\n        TaskEvent::Created { id, title } => state.insert(*id, title),\n        TaskEvent::Completed { id } => state.complete(*id),\n    }\n}"),
        ]),
        doc_section(locale, "modes", "Progressive operating modes", "Modos operativos progresivos", vec![
            steps(locale, &[
                ("Static only", "No event history is required. Author complete content and deterministic output.", "Sólo estático", "No se requiere historia de eventos. Crea contenido completo y salida determinista."),
                ("Local history", "Append events locally and prove live-versus-replay projection parity.", "Historia local", "Añade eventos localmente y prueba la paridad de proyección live-versus-replay."),
                ("Durable outbox", "Record commands and pending effects before transport or retry.", "Outbox durable", "Registra comandos y efectos pendientes antes de transporte o retry."),
                ("Verified sync", "Accept remote history only through typed, contiguous, verified receipts.", "Sync verificado", "Acepta historia remota sólo mediante recibos tipados, contiguos y verificados."),
            ]),
        ]),
        doc_section(locale, "replay", "Replay parity", "Paridad de replay", vec![
            paragraph(locale, "Every event-native starter should test three paths: live append, restore from snapshot plus tail, and replay from genesis. Their canonical projection bytes must match.", "Cada starter event-native debe probar tres rutas: append live, restore desde snapshot más tail y replay desde genesis. Sus bytes canónicos de proyección deben coincidir."),
            code_block(locale, "rust", "assert_eq!(live_state, replay(&events));\nassert_eq!(live_state, restore(snapshot, &tail));"),
        ]),
        doc_section(locale, "effects", "Effects become evidence", "Los efectos se convierten en evidencia", vec![
            paragraph(locale, "Reducers do not call clocks, randomness, networks, filesystems, or models. They emit a command; an effect runner performs external work; the observed result returns as an event or receipt.", "Los reducers no llaman relojes, random, redes, filesystems o modelos. Emiten un comando; un effect runner realiza el trabajo externo; el resultado observado vuelve como evento o recibo."),
        ]),
    ].into_view()
}

fn typed_content(locale: Locale) -> View {
    vec![
        doc_section(locale, "collections", "Typed collections", "Colecciones tipadas", vec![
            paragraph(locale, "pliego-content discovers bounded Markdown, JSON, and TOML inputs, parses them into application types, and preserves stable source identities for diagnostics and build evidence.", "pliego-content descubre entradas limitadas de Markdown, JSON y TOML, las parsea hacia tipos de la aplicación y preserva identidades de fuente estables para diagnósticos y evidencia del build."),
            code_block(locale, "rust", "#[derive(serde::Deserialize)]\nstruct Article {\n    title: String,\n    summary: String,\n    published: String,\n}\n\nlet articles = collection.load::<Article>()?;"),
        ]),
        doc_section(locale, "markdown", "Safe Markdown", "Markdown seguro", vec![
            paragraph(locale, "CommonMark rendering escapes raw HTML by default, limits input bytes and nesting, and reports the source path with the failing field. Content is data, not an executable template language.", "El render CommonMark escapa HTML crudo por defecto, limita bytes y nesting y reporta la ruta fuente junto al campo que falla. El contenido es data, no un lenguaje de templates ejecutable."),
        ]),
        doc_section(locale, "limits", "Bound every input", "Limita cada entrada", vec![
            definition_list(locale, &[
                ("Files", "Maximum discovered entries and portable relative paths", "Máximo de entradas descubiertas y rutas relativas portables"),
                ("Bytes", "Per-file and collection-wide byte ceilings", "Límites de bytes por archivo y colección"),
                ("Shape", "deny_unknown_fields for public content contracts", "deny_unknown_fields para contratos públicos de contenido"),
                ("Identity", "Stable path and content digest in diagnostics", "Ruta estable y digest del contenido en diagnósticos"),
            ]),
        ]),
        doc_section(locale, "errors", "Actionable content errors", "Errores de contenido accionables", vec![
            code_block(locale, "text", "PLG-CNT-004 content contract\ncontent/articles/launch.toml: unknown field `titel`\nExpected: title, summary, published\nNext: correct the field and save; pliego dev will rebuild."),
        ]),
    ].into_view()
}

fn browser_runtime(locale: Locale) -> View {
    vec![
        doc_section(locale, "resume", "Resume owned behavior", "Reanuda comportamiento controlado", vec![
            paragraph(locale, "The site package emits complete HTML. The optional client package compiles to WASM and resumes only the stateful boundary marked by the document. It does not hydrate or diff the entire page.", "El paquete del sitio emite HTML completo. El paquete de cliente opcional compila a WASM y reanuda únicamente el límite con estado marcado por el documento. No hidrata ni compara la página completa."),
            code_block(locale, "toml", "[client]\npackage = \"my-app-client\"\nwasm_name = \"my_app_client\"\nbindgen_output = \"target/my-app-client/pkg\""),
        ]),
        doc_section(locale, "adapters", "External adapter contract", "Contrato de adapters externos", vec![
            code_block(locale, "javascript", "export default {\n  version: 1,\n  mount(context) {\n    const instance = createLibrary(context.root, context.props);\n    return {\n      update(next) { instance.update(next); },\n      unmount() { instance.destroy(); }\n    };\n  }\n};"),
            paragraph(locale, "GSAP, Lenis, Three.js, WebGL, and other mature browser libraries remain native JavaScript. PliegoRS owns when they load, which capabilities admit them, and how they are cancelled and cleaned up.", "GSAP, Lenis, Three.js, WebGL y otras librerías maduras permanecen como JavaScript nativo. PliegoRS controla cuándo cargan, qué capacidades las admiten y cómo se cancelan y limpian."),
        ]),
        doc_section(locale, "policy", "Admission policy", "Política de admisión", vec![
            definition_list(locale, &[
                ("trigger", "load, visible, interaction, or explicit intent", "load, visible, interaction o intención explícita"),
                ("capability tier", "Device and renderer requirements checked before import", "Requisitos del dispositivo y renderer comprobados antes del import"),
                ("Save-Data", "Can deny optional media or runtime work", "Puede negar media o trabajo de runtime opcional"),
                ("reduced motion", "Preserves content and actions while removing nonessential motion", "Preserva contenido y acciones mientras elimina movimiento no esencial"),
            ]),
        ]),
        doc_section(locale, "cleanup", "Cleanup is automatic", "El cleanup es automático", vec![
            paragraph(locale, "Unmount, route replacement, policy changes, aborted imports, and failed updates cannot leave an adapter alive. Async cleanup completes before the next generation mounts.", "Unmount, reemplazo de ruta, cambios de policy, imports abortados y updates fallidos no pueden dejar un adapter vivo. El cleanup asíncrono termina antes de montar la siguiente generación."),
        ]),
    ].into_view()
}

fn adaptive_assets(locale: Locale) -> View {
    vec![
        doc_section(locale, "manifest", "Asset manifest", "Manifest de assets", vec![
            paragraph(locale, "An asset manifest records source identity, variants, media type, dimensions, duration, bytes, digest, fallback relationships, and the route or scene that consumes them.", "Un manifest de assets registra identidad de fuente, variantes, tipo de medio, dimensiones, duración, bytes, digest, relaciones de fallback y la ruta o escena que los consume."),
            code_block(locale, "json", "{\n  \"id\": \"hero-fold\",\n  \"source\": \"media/fold-master.png\",\n  \"variants\": [\n    { \"path\": \"media/fold.avif\", \"width\": 1672, \"bytes\": 19746 },\n    { \"path\": \"media/fold.webp\", \"width\": 1672, \"bytes\": 59306 }\n  ]\n}"),
        ]),
        doc_section(locale, "profiles", "Reproducible profiles", "Perfiles reproducibles", vec![
            definition_list(locale, &[
                ("image", "AVIF/WebP variants, dimensions, quality, fit, and fallback", "Variantes AVIF/WebP, dimensiones, calidad, fit y fallback"),
                ("video", "Codec, bitrate, dimensions, duration, poster, and reduced-data alternative", "Codec, bitrate, dimensiones, duración, poster y alternativa de datos reducidos"),
                ("font", "Subset, axes, preload policy, license, and fallback metrics", "Subset, ejes, política de preload, licencia y métricas de fallback"),
                ("3D", "Geometry, textures, compression, renderer tier, and static fallback", "Geometría, texturas, compresión, tier de renderer y fallback estático"),
            ]),
        ]),
        doc_section(locale, "budgets", "Device budgets", "Presupuestos por dispositivo", vec![
            paragraph(locale, "Budgets are evaluated by route and capability tier. The first viewport, complete document, scene memory, and long-task behavior remain separate evidence instead of being collapsed into one marketing score.", "Los presupuestos se evalúan por ruta y capability tier. El primer viewport, documento completo, memoria de escena y long tasks permanecen como evidencia separada en vez de colapsarse en un score de marketing."),
        ]),
        doc_section(locale, "fallbacks", "Fallbacks are part of the design", "Los fallbacks son parte del diseño", vec![
            paragraph(locale, "Every optional 3D, video, or motion surface defines a meaningful static or lower-tier representation. Save-Data and reduced motion never produce a blank composition.", "Cada superficie opcional de 3D, video o motion define una representación estática o de menor tier con significado. Save-Data y movimiento reducido nunca producen una composición vacía."),
        ]),
    ].into_view()
}

fn errors_and_diagnostics(locale: Locale) -> View {
    vec![
        doc_section(locale, "browser", "Build failures in the browser", "Fallos de build en el navegador", vec![
            paragraph(locale, "pliego dev starts even when the initial site compilation fails. Document requests receive a branded HTTP 500 diagnostic page containing the stable code, escaped compiler output, and the next action. Saving a fix rebuilds and reloads the page automatically.", "pliego dev inicia incluso cuando falla la compilación inicial del sitio. Las solicitudes de documentos reciben una página diagnóstica HTTP 500 de marca con el código estable, salida del compilador escapada y la siguiente acción. Guardar un fix recompila y recarga la página automáticamente."),
            note(locale, "Last valid output", "Static assets from the last valid build remain available, but HTML documents display the current failure so a stale page cannot hide a broken build.", "Última salida válida", "Los assets estáticos del último build válido permanecen disponibles, pero los documentos HTML muestran el fallo actual para que una página vieja no oculte un build roto."),
        ]),
        doc_section(locale, "codes", "Stable diagnostic codes", "Códigos diagnósticos estables", vec![
            paragraph(locale, "A diagnostic code identifies the failing contract, not the wording of one compiler version. Tooling may match the code and exit status; humans receive the message and recovery action.", "Un código diagnóstico identifica el contrato que falla, no la redacción de una versión del compilador. El tooling puede asociar el código y estado de salida; las personas reciben el mensaje y la acción de recuperación."),
            code_block(locale, "text", "PLG-BLD-001 / BUILD FAILED\nThe site package did not compile.\n\nerror[E0425]: cannot find value `titel` in this scope\n\nNEXT\nCorrect the reported source error and save the file."),
        ]),
        doc_section(locale, "http", "HTTP failure surfaces", "Superficies de fallo HTTP", vec![
            definition_list(locale, &[
                ("404", "The project's authored /404.html, or a branded framework fallback", "El /404.html con autoría del proyecto o un fallback de marca del framework"),
                ("405", "Method rejected with an Allow header", "Método rechazado con header Allow"),
                ("414", "Oversized request target rejected before filesystem resolution", "Request target sobredimensionado rechazado antes de resolver el filesystem"),
                ("500", "Current development build failure or an unreadable document artifact", "Fallo actual del build de desarrollo o artefacto de documento ilegible"),
                ("503", "Bounded request queue is full; Retry-After is returned", "La cola limitada está llena; se devuelve Retry-After"),
            ]),
        ]),
        doc_section(locale, "recovery", "Recovery workflow", "Flujo de recuperación", vec![
            steps(locale, &[
                ("Read the code", "Start with PLG-BLD, PLG-ENV, PLG-ART, or the category shown in the browser.", "Lee el código", "Comienza con PLG-BLD, PLG-ENV, PLG-ART o la categoría mostrada en el navegador."),
                ("Fix the first cause", "Compiler output is ordered; later errors may be consequences.", "Corrige la primera causa", "La salida del compilador está ordenada; errores posteriores pueden ser consecuencias."),
                ("Save and observe", "The watcher rebuilds and the diagnostic page reloads without restarting pliego dev.", "Guarda y observa", "El watcher recompila y la página diagnóstica recarga sin reiniciar pliego dev."),
                ("Verify production", "Run pliego build and pliego inspect after the development page is green.", "Verifica producción", "Ejecuta pliego build y pliego inspect después de que la página de desarrollo esté verde."),
            ]),
        ]),
    ].into_view()
}

fn build_and_deploy(locale: Locale) -> View {
    vec![
        doc_section(locale, "build", "Build production bytes", "Compila bytes de producción", vec![
            code_block(locale, "shell", "pliego check\npliego build\npliego inspect\npliego preview 4400"),
            paragraph(locale, "build compiles the optional WASM client in release mode, runs wasm-bindgen, executes the native site package, stages output atomically, and verifies the emitted ledger. preview refuses output that cannot prove its file list.", "build compila el cliente WASM opcional en modo release, ejecuta wasm-bindgen y el paquete nativo, hace staging atómico y verifica el ledger emitido. preview rechaza salida incapaz de probar su lista de archivos."),
        ]),
        doc_section(locale, "ledger", "Inspect the artifact", "Inspecciona el artefacto", vec![
            paragraph(locale, "pliego.build.json is the ownership boundary between source and deployment. It binds route files, assets, hashes, source revision, toolchain identity, and the output contract consumed by inspect, preview, and release workflows.", "pliego.build.json es el límite de propiedad entre fuente y despliegue. Vincula archivos de rutas, assets, hashes, revisión de fuente, identidad de toolchain y el contrato de salida consumido por inspect, preview y workflows de release."),
        ]),
        doc_section(locale, "releases", "Release selection", "Selección de releases", vec![
            paragraph(locale, "GitHub Releases is the canonical distribution channel. Production targets are Linux x86_64 and ARM64; macOS and Windows artifacts support development. Versioned archives, sidecars, SHA256SUMS, and the signed release manifest must agree.", "GitHub Releases es el canal canónico de distribución. Los targets de producción son Linux x86_64 y ARM64; los artefactos macOS y Windows soportan desarrollo. Archives versionados, sidecars, SHA256SUMS y el manifest firmado deben coincidir."),
            code_block(locale, "shell", "# Explicit version selection\n./install.sh --version 0.1.0\n\n# Explicit latest-channel opt-in\n./install.sh --channel latest"),
        ]),
        doc_section(locale, "deploy", "Deploy the static output", "Despliega la salida estática", vec![
            paragraph(locale, "Deploy the contents of target/site to any origin that preserves paths, MIME types, immutable asset caching, the authored 404 document, and clean-route fallback. The framework does not require a PliegoRS application server.", "Despliega el contenido de target/site en cualquier origen que preserve rutas, MIME types, cache inmutable de assets, el documento 404 con autoría y fallback de rutas limpias. El framework no requiere un servidor de aplicación PliegoRS."),
            note(locale, "Verify after upload", "Compare uploaded bytes against the ledger or release manifest. A successful local build is not evidence that an origin serves the same artifact.", "Verifica después del upload", "Compara los bytes subidos contra el ledger o manifest de release. Un build local exitoso no prueba que un origen sirva el mismo artefacto."),
        ]),
    ].into_view()
}

fn licensing(locale: Locale) -> View {
    vec![
        doc_section(locale, "apache", "Apache License 2.0", "Licencia Apache 2.0", vec![
            paragraph(locale, "PliegoRS framework source is licensed under Apache-2.0. You may use, modify, and distribute it under that license, subject to its copyright, notice, patent, and redistribution terms. The LICENSE file is the controlling text.", "El código fuente del framework PliegoRS está licenciado bajo Apache-2.0. Puedes usarlo, modificarlo y distribuirlo bajo esa licencia, sujeto a sus términos de copyright, avisos, patentes y redistribución. El archivo LICENSE es el texto rector."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/LICENSE", "Read LICENSE", "Leer LICENSE"),
                ("https://github.com/celiumsai/pliegors/blob/main/NOTICE", "Read NOTICE", "Leer NOTICE"),
            ]),
        ]),
        doc_section(locale, "starters", "Generated starters", "Starters generados", vec![
            paragraph(locale, "Official starter source is provided under Apache-2.0 and includes its license. Your application code, content, brand, and original assets remain yours; choose and document the license that fits your project before publication. Third-party fonts and media retain their own notices.", "El código de los starters oficiales se ofrece bajo Apache-2.0 e incluye su licencia. El código, contenido, marca y assets originales de tu aplicación siguen siendo tuyos; elige y documenta la licencia adecuada antes de publicar. Las fuentes y medios de terceros conservan sus propios avisos."),
        ]),
        doc_section(locale, "trademarks", "Names and marks", "Nombres y marcas", vec![
            paragraph(locale, "Apache-2.0 does not grant permission to use PliegoRS or Celiums names, logos, or marks to imply endorsement. Descriptive references such as “Built with PliegoRS” are governed by the repository trademark policy.", "Apache-2.0 no concede permiso para usar nombres, logos o marcas de PliegoRS o Celiums para implicar respaldo. Referencias descriptivas como “Built with PliegoRS” se rigen por la política de marcas del repositorio."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/TRADEMARKS.md", "Trademark policy", "Política de marcas")]),
        ]),
        doc_section(locale, "project-policy", "Public project policy", "Política pública del proyecto", vec![
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/SECURITY.md", "Report a vulnerability", "Reportar una vulnerabilidad"),
                ("https://github.com/celiumsai/pliegors/blob/main/CONTRIBUTING.md", "Contributing guide", "Guía de contribución"),
                ("https://github.com/celiumsai/pliegors/blob/main/CODE_OF_CONDUCT.md", "Code of conduct", "Código de conducta"),
                ("https://github.com/celiumsai/pliegors/blob/main/SUPPORT.md", "Support policy", "Política de soporte"),
            ]),
            note(locale, "Not legal advice", "This guide explains the repository's intended licensing surface. The license and policy files control; obtain legal advice for your own distribution obligations.", "No es asesoría legal", "Esta guía explica la superficie de licenciamiento prevista del repositorio. La licencia y los archivos de política son rectores; obtén asesoría legal para tus propias obligaciones de distribución."),
        ]),
    ].into_view()
}

fn command_table(locale: Locale) -> View {
    let commands = [
        (
            "pliego new <path>",
            "Create the default starter transactionally",
            "Crea el starter predeterminado de forma transaccional",
        ),
        (
            "pliego templates",
            "List maintained starter IDs and capabilities",
            "Lista IDs y capacidades de starters mantenidos",
        ),
        (
            "pliego check",
            "Validate project and toolchain contracts",
            "Valida contratos del proyecto y toolchain",
        ),
        (
            "pliego build",
            "Produce and verify production output",
            "Produce y verifica salida de producción",
        ),
        (
            "pliego dev",
            "Build, watch, diagnose, and reload",
            "Compila, observa, diagnostica y recarga",
        ),
        (
            "pliego preview",
            "Serve an existing verified build",
            "Sirve un build verificado existente",
        ),
        (
            "pliego inspect",
            "Report routes, files, bytes, and ledger validity",
            "Reporta rutas, archivos, bytes y validez del ledger",
        ),
        (
            "pliego version",
            "Print the CLI version",
            "Imprime la versión del CLI",
        ),
    ];
    let mut list = el("div").class("rs-doc-command-list");
    for (command, en, es) in commands {
        list = list.child(
            el("div")
                .child(el("code").child(command))
                .child(el("p").child(localized(locale, en, es))),
        );
    }
    list.into_view()
}

fn doc_section(
    locale: Locale,
    id: &str,
    title_en: &str,
    title_es: &str,
    children: Vec<View>,
) -> View {
    let mut section = el("section")
        .class("rs-doc-section")
        .id(id)
        .child(el("h2").child(localized(locale, title_en, title_es)));
    for child in children {
        section = section.child(child);
    }
    section.into_view()
}

fn paragraph(locale: Locale, en: &str, es: &str) -> View {
    el("p").child(localized(locale, en, es)).into_view()
}

fn code_block(locale: Locale, language: &str, code: &str) -> View {
    let id = format!("code-{:016x}", stable_hash(code));
    el("div")
        .class("rs-code-block")
        .child(
            el("div")
                .class("rs-code-block__head")
                .child(el("span").child(language.to_owned()))
                .child(
                    el("button")
                        .attr("type", "button")
                        .attr("data-doc-copy", "")
                        .attr("data-copy-target", id.clone())
                        .attr("data-copy-label", localized(locale, "Copy", "Copiar"))
                        .attr("data-copied-label", localized(locale, "Copied", "Copiado"))
                        .attr(
                            "data-copy-failed-label",
                            localized(locale, "Copy failed", "No se pudo copiar"),
                        )
                        .child(localized(locale, "Copy", "Copiar")),
                ),
        )
        .child(el("pre").child(el("code").id(id).child(code.to_owned())))
        .into_view()
}

fn note(locale: Locale, title_en: &str, body_en: &str, title_es: &str, body_es: &str) -> View {
    el("aside")
        .class("rs-doc-note")
        .child(el("strong").child(localized(locale, title_en, title_es)))
        .child(el("p").child(localized(locale, body_en, body_es)))
        .into_view()
}

fn steps(locale: Locale, items: &[(&str, &str, &str, &str)]) -> View {
    let mut list = el("ol").class("rs-doc-steps");
    for (index, (title_en, body_en, title_es, body_es)) in items.iter().enumerate() {
        list = list.child(
            el("li")
                .child(el("span").child(format!("{:02}", index + 1)))
                .child(
                    el("div")
                        .child(el("h3").child(localized(locale, title_en, title_es)))
                        .child(el("p").child(localized(locale, body_en, body_es))),
                ),
        );
    }
    list.into_view()
}

fn definition_list(locale: Locale, items: &[(&str, &str, &str)]) -> View {
    let mut list = el("dl").class("rs-doc-definitions");
    for (term, en, es) in items {
        list = list.child(
            el("div")
                .child(el("dt").child(*term))
                .child(el("dd").child(localized(locale, en, es))),
        );
    }
    list.into_view()
}

fn link_list(locale: Locale, items: &[(&str, &str, &str)]) -> View {
    let mut list = el("div").class("rs-doc-links");
    for (href, en, es) in items {
        let target = if href.starts_with('/') {
            locale_path(locale, href)
        } else {
            (*href).to_owned()
        };
        list = list.child(
            el("a")
                .attr("href", target)
                .child(localized(locale, en, es))
                .child(el("span").attr("aria-hidden", "true").child("↗")),
        );
    }
    list.into_view()
}

fn outline(slug: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    match slug {
        "getting-started" => vec![
            ("requirements", "Before you begin", "Antes de comenzar"),
            ("install", "Install the CLI", "Instalar el CLI"),
            ("create", "Create and run", "Crear y ejecutar"),
            ("next", "Where to go next", "Qué sigue"),
        ],
        "project-structure" => vec![
            ("tree", "The default tree", "El árbol predeterminado"),
            ("manifest", "pliego.toml", "pliego.toml"),
            ("packages", "Site and client", "Sitio y cliente"),
            ("output", "Output artifact", "Artefacto de salida"),
        ],
        "cli" => vec![
            ("commands", "Commands", "Comandos"),
            (
                "development",
                "Development servers",
                "Servidores de desarrollo",
            ),
            ("diagnostics", "JSON diagnostics", "Diagnósticos JSON"),
            ("exit-codes", "Exit codes", "Códigos de salida"),
        ],
        "routing-and-pages" => vec![
            ("page", "Author a page", "Crear una página"),
            ("routes", "Route normalization", "Normalización de rutas"),
            ("head", "Metadata", "Metadata"),
            ("errors", "Error pages", "Páginas de error"),
        ],
        "views" => vec![
            ("composition", "Semantic HTML", "HTML semántico"),
            ("macro", "Typed macro", "Macro tipada"),
            ("static-first", "Useful HTML", "HTML útil"),
            ("ownership", "Mounted ownership", "Propiedad montada"),
        ],
        "events-and-folds" => vec![
            ("model", "Facts and projections", "Hechos y proyecciones"),
            ("modes", "Operating modes", "Modos operativos"),
            ("replay", "Replay parity", "Paridad de replay"),
            ("effects", "Effects", "Efectos"),
        ],
        "content" => vec![
            ("collections", "Typed collections", "Colecciones tipadas"),
            ("markdown", "Safe Markdown", "Markdown seguro"),
            ("limits", "Input limits", "Límites de entrada"),
            ("errors", "Content errors", "Errores de contenido"),
        ],
        "browser-runtime" => vec![
            ("resume", "Resume behavior", "Reanudar comportamiento"),
            ("adapters", "Adapter contract", "Contrato de adapters"),
            ("policy", "Admission policy", "Política de admisión"),
            ("cleanup", "Cleanup", "Cleanup"),
        ],
        "assets" => vec![
            ("manifest", "Asset manifest", "Manifest de assets"),
            ("profiles", "Profiles", "Perfiles"),
            ("budgets", "Device budgets", "Presupuestos"),
            ("fallbacks", "Fallbacks", "Fallbacks"),
        ],
        "errors-and-diagnostics" => vec![
            ("browser", "Browser failures", "Fallos en navegador"),
            ("codes", "Diagnostic codes", "Códigos diagnósticos"),
            ("http", "HTTP failures", "Fallos HTTP"),
            ("recovery", "Recovery", "Recuperación"),
        ],
        "build-and-deploy" => vec![
            ("build", "Build", "Build"),
            ("ledger", "Artifact ledger", "Ledger del artefacto"),
            ("releases", "Releases", "Releases"),
            ("deploy", "Deploy", "Desplegar"),
        ],
        "licensing" => vec![
            ("apache", "Apache-2.0", "Apache-2.0"),
            ("starters", "Generated starters", "Starters generados"),
            ("trademarks", "Names and marks", "Nombres y marcas"),
            ("project-policy", "Project policy", "Política del proyecto"),
        ],
        _ => Vec::new(),
    }
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn localized<'a>(locale: Locale, en: &'a str, es: &'a str) -> &'a str {
    if locale.is_spanish() { es } else { en }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn documentation_registry_has_unique_routes_and_renderers() {
        let mut slugs = BTreeSet::new();
        for topic in TOPICS {
            assert!(slugs.insert(topic.slug));
            assert!(article(Locale::En, topic.slug).is_ok());
            assert!(article(Locale::Es, topic.slug).is_ok());
        }
        assert_eq!(TOPICS.len(), 12);
    }

    #[test]
    fn code_ids_are_stable_and_content_addressed() {
        assert_eq!(stable_hash("pliego dev"), stable_hash("pliego dev"));
        assert_ne!(stable_hash("pliego dev"), stable_hash("pliego build"));
    }
}
