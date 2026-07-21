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
        summary_en: "Use the project, diagnostics, upgrade, release-inspection, and voluntary telemetry command surfaces.",
        summary_es: "Usa las superficies de proyecto, diagnóstico, upgrade, inspección de releases y telemetría voluntaria.",
    },
    DocTopic {
        slug: "developer-loop",
        group_en: "Start",
        group_es: "Inicio",
        title_en: "Developer loop",
        title_es: "Bucle de desarrollo",
        summary_en: "Follow native file events through typed HMR, causal build graphs, artifact explanations, and recovery without losing the last valid output.",
        summary_es: "Sigue eventos nativos de archivos mediante HMR tipado, grafos causales, explicaciones de artefactos y recuperación sin perder la última salida válida.",
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
        slug: "schemas-and-snapshots",
        group_en: "State",
        group_es: "Estado",
        title_en: "Schemas and snapshots",
        title_es: "Schemas y snapshots",
        summary_en: "Version events, seal adjacent upcasters, bind projection identity, and restore snapshots only against an exact compatible contract.",
        summary_es: "Versiona eventos, sella upcasters adyacentes, vincula la identidad de proyección y restaura snapshots sólo contra un contrato compatible exacto.",
    },
    DocTopic {
        slug: "hyphae-sync",
        group_en: "State",
        group_es: "Estado",
        title_en: "Hyphae verified sync",
        title_es: "Sync verificado con Hyphae",
        summary_en: "Append and replay durable history through signed attestations, fixed snapshots, authority policy, and consuming verification types.",
        summary_es: "Añade y reproduce historia durable mediante attestations firmadas, snapshots fijos, política de autoridad y tipos de verificación consumibles.",
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
        slug: "dom-lifecycle",
        group_en: "Runtime",
        group_es: "Runtime",
        title_en: "DOM ownership",
        title_es: "Propiedad del DOM",
        summary_en: "Own mounted ranges, reconcile keyed children, adopt exact SSR output, and dispose listeners, effects, adapters, and nodes deterministically.",
        summary_es: "Controla rangos montados, reconcilia hijos keyed, adopta salida SSR exacta y elimina listeners, efectos, adapters y nodos de forma determinista.",
    },
    DocTopic {
        slug: "native-runtime",
        group_en: "Runtime",
        group_es: "Runtime",
        title_en: "Native runtime preview",
        title_es: "Preview del runtime nativo",
        summary_en: "Evaluate the unreleased sealed router, bounded request lifecycle, middleware, authored errors, and three SSR modes from main.",
        summary_es: "Evalúa desde main el router sellado, lifecycle limitado de requests, middleware, errores con autoría y tres modos SSR aún no liberados.",
    },
    DocTopic {
        slug: "opensdk",
        group_en: "OpenSDK",
        group_es: "OpenSDK",
        title_en: "OpenSDK preview",
        title_es: "Preview de OpenSDK",
        summary_en: "Evaluate the unreleased extension boundary from main, understand admission, capabilities, effects, and its governance status.",
        summary_es: "Evalúa desde main el límite de extensiones aún no liberado y entiende admisión, capabilities, efectos y su estado de gobernanza.",
    },
    DocTopic {
        slug: "opensdk-components",
        group_en: "OpenSDK",
        group_es: "OpenSDK",
        title_en: "Components and languages",
        title_es: "Componentes y lenguajes",
        summary_en: "Run the typed Wasm Component host and compare the bounded Rust, TypeScript, and Python conformance implementations.",
        summary_es: "Ejecuta el host tipado de Wasm Components y compara las implementaciones limitadas de conformidad en Rust, TypeScript y Python.",
    },
    DocTopic {
        slug: "browser-framework-conformance",
        group_en: "OpenSDK",
        group_es: "OpenSDK",
        title_en: "Browser framework conformance",
        title_es: "Conformidad de frameworks web",
        summary_en: "Integrate React, Svelte, and Lit through explicit Custom Element and adapter lifecycles with measurable cleanup.",
        summary_es: "Integra React, Svelte y Lit mediante lifecycles explícitos de Custom Elements y adapters con cleanup medible.",
    },
    DocTopic {
        slug: "opensdk-tooling",
        group_en: "OpenSDK",
        group_es: "OpenSDK",
        title_en: "Tooling protocol",
        title_es: "Protocolo de tooling",
        summary_en: "Negotiate editor and MCP clients over bounded JSON-RPC without granting ambient project or network authority.",
        summary_es: "Negocia clientes de editor y MCP sobre JSON-RPC limitado sin conceder autoridad ambiental sobre el proyecto o la red.",
    },
    DocTopic {
        slug: "opensdk-compatibility",
        group_en: "OpenSDK",
        group_es: "OpenSDK",
        title_en: "Compatibility and deprecation",
        title_es: "Compatibilidad y deprecación",
        summary_en: "Read the machine-verifiable support matrix, stability tiers, migration requirements, and provider-neutral source boundary.",
        summary_es: "Lee la matriz verificable de soporte, niveles de estabilidad, requisitos de migración y límite de fuente independiente del proveedor.",
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
        slug: "artifact-trust",
        group_en: "Delivery",
        group_es: "Entrega",
        title_en: "Artifact trust",
        title_es: "Confianza de artefactos",
        summary_en: "Understand portable namespaces, exact source capture, staged publication, build receipts, causal graphs, and fail-closed verification.",
        summary_es: "Entiende namespaces portables, captura exacta de fuentes, publicación por staging, recibos de build, grafos causales y verificación fail-closed.",
    },
    DocTopic {
        slug: "release-trust",
        group_en: "Delivery",
        group_es: "Entrega",
        title_en: "Release trust",
        title_es: "Confianza de releases",
        summary_en: "Verify deterministic archives, signatures, attestations, provenance, and release-only promotion evidence.",
        summary_es: "Verifica archives deterministas, firmas, attestations, provenance y evidencia de promoción exclusiva del release.",
    },
    DocTopic {
        slug: "performance-evidence",
        group_en: "Operate",
        group_es: "Operación",
        title_en: "Performance evidence",
        title_es: "Evidencia de rendimiento",
        summary_en: "Reproduce benchmark observations, read their hardware context, and keep measurements separate from claims.",
        summary_es: "Reproduce observaciones de benchmarks, lee su contexto de hardware y separa mediciones de afirmaciones.",
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
        slug: "telemetry",
        group_en: "Operate",
        group_es: "Operación",
        title_en: "Voluntary telemetry",
        title_es: "Telemetría voluntaria",
        summary_en: "Control a disabled-by-default, local-only usage journal with an exact field allowlist and user-owned deletion.",
        summary_es: "Controla un journal de uso local, desactivado por defecto, con allowlist exacta y eliminación controlada por el usuario.",
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
        slug: "crate-reference",
        group_en: "Reference",
        group_es: "Referencia",
        title_en: "Crates and API",
        title_es: "Crates y API",
        summary_en: "Choose the public crate that owns each contract, generate exact-version Rustdoc, and avoid depending on implementation-only internals.",
        summary_es: "Elige el crate público que controla cada contrato, genera Rustdoc de versión exacta y evita depender de internals de implementación.",
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
        let group = localized(locale, topic.group_en, topic.group_es);
        topics = topics.child(
            el("a")
                .class("rs-doc-card")
                .attr(
                    "href",
                    locale_path(locale, &format!("/docs/{}", topic.slug)),
                )
                .attr("data-docs-item", "")
                .attr(
                    "data-search",
                    format!("{group} {title} {summary}").to_lowercase(),
                )
                .child(el("span").child(format!("{:02}", index + 1)))
                .child(el("p").class("utility-label").child(group))
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
                .attr("aria-labelledby", "docs-status-title")
                .child(
                    el("div")
                        .child(
                            el("p")
                                .class("utility-label")
                                .child("RELEASE / 0.0.2 + G1 / PREVIEW"),
                        )
                        .child(el("h2").id("docs-status-title").child(localized(
                            locale,
                            "Published framework. Unreleased server and extension boundaries.",
                            "Framework publicado. Límites de servidor y extensiones aún no liberados.",
                        )))
                        .child(el("p").child(localized(
                            locale,
                            "The fifteen 0.0.2 crates are the current installable release. pliego-router, pliego-runtime, and pliego-sdk 0.1.0-preview.1 are executable on main with Rust 1.86, but are not on crates.io and remain outside the released API.",
                            "Los quince crates 0.0.2 forman el release instalable actual. pliego-router, pliego-runtime y pliego-sdk 0.1.0-preview.1 son ejecutables en main con Rust 1.86, pero no están en crates.io y permanecen fuera de la API liberada.",
                        ))),
                )
                .child(link_list(
                    locale,
                    &[
                        ("https://pliegors.dev/capabilities.json", "Inspect current capabilities", "Inspeccionar capacidades actuales"),
                        ("/docs/native-runtime", "Evaluate the native runtime", "Evaluar el runtime nativo"),
                        ("/docs/opensdk", "Evaluate OpenSDK", "Evaluar OpenSDK"),
                        ("/changelog", "Read the release boundary", "Leer el límite de release"),
                    ],
                )),
        )
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
        "developer-loop" => developer_loop(locale),
        "routing-and-pages" => routing_and_pages(locale),
        "views" => views(locale),
        "events-and-folds" => events_and_folds(locale),
        "schemas-and-snapshots" => schemas_and_snapshots(locale),
        "hyphae-sync" => hyphae_sync(locale),
        "content" => typed_content(locale),
        "browser-runtime" => browser_runtime(locale),
        "dom-lifecycle" => dom_lifecycle(locale),
        "native-runtime" => native_runtime(locale),
        "opensdk" => opensdk(locale),
        "opensdk-components" => opensdk_components(locale),
        "browser-framework-conformance" => browser_framework_conformance(locale),
        "opensdk-tooling" => opensdk_tooling(locale),
        "opensdk-compatibility" => opensdk_compatibility(locale),
        "assets" => adaptive_assets(locale),
        "artifact-trust" => artifact_trust(locale),
        "release-trust" => release_trust(locale),
        "performance-evidence" => performance_evidence(locale),
        "errors-and-diagnostics" => errors_and_diagnostics(locale),
        "telemetry" => telemetry(locale),
        "build-and-deploy" => build_and_deploy(locale),
        "crate-reference" => crate_reference(locale),
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
                    ("Rust", "1.85.0 for released v0.0.2 artifacts; 1.86.0 for current main and OpenSDK", "1.85.0 para artefactos v0.0.2 liberados; 1.86.0 para main actual y OpenSDK"),
                    ("Targets", "Linux x64/ARM64 for production; macOS and Windows for development", "Linux x64/ARM64 para producción; macOS y Windows para desarrollo"),
                    ("Source", "crates.io packages, GitHub Releases, and the canonical celiumsai/pliegors repository", "Paquetes de crates.io, GitHub Releases y el repositorio canónico celiumsai/pliegors"),
                ]),
            ],
        ),
        doc_section(
            locale,
            "install",
            "Install the CLI",
            "Instalar el CLI",
            vec![
                note(locale, "Current release", "Install the exact 0.0.2 CLI from crates.io. The generated project pins every released PliegoRS crate to that same exact version.", "Release actual", "Instala el CLI 0.0.2 exacto desde crates.io. El proyecto generado fija cada crate liberado de PliegoRS a esa misma versión exacta."),
                code_block(locale, "shell", "cargo install pliego-cli --version 0.0.2 --locked\npliego version"),
                paragraph(locale, "Release installers are downloaded to disk, verified, and then executed. PliegoRS never documents piping an unverified network response directly into a shell.", "Los instaladores de release se descargan a disco, se verifican y después se ejecutan. PliegoRS nunca documenta enviar una respuesta de red sin verificar directamente a un shell."),
                link_list(locale, &[("https://github.com/celiumsai/pliegors/releases/tag/v0.0.2", "Download and verify the signed release bundle", "Descargar y verificar el bundle firmado"), ("https://github.com/celiumsai/pliegors/blob/main/docs/27-distribution-and-release.md", "Read the distribution contract", "Leer el contrato de distribución")]),
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
            code_block(locale, "toml", "[project]\nid = \"my-app\"\nname = \"My App\"\nsite_package = \"my-app\"\noutput = \"target/site\""),
            paragraph(locale, "Generated paths must stay inside the project, use portable names, and avoid the source tree. The nearest pliego.toml defines the active project for every CLI command.", "Las rutas generadas deben permanecer dentro del proyecto, usar nombres portables y evitar el árbol de fuentes. El pliego.toml más cercano define el proyecto activo para cada comando del CLI."),
        ]),
        doc_section(locale, "packages", "Site and client packages", "Paquetes de sitio y cliente", vec![
            definition_list(locale, &[
                ("id", "A stable portable identity that owns the artifact lineage", "Una identidad portable estable que posee el linaje de artefactos"),
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

fn developer_loop(locale: Locale) -> View {
    vec![
        doc_section(locale, "watch", "Native events, bounded rebuilds", "Eventos nativos, rebuilds limitados", vec![
            paragraph(locale, "pliego dev watches authored inputs through the operating system, debounces event bursts, ignores generated roots, and never follows source symlinks. A failed rebuild keeps the last verified site available while diagnostics remain live.", "pliego dev observa entradas de autoría mediante el sistema operativo, agrupa ráfagas de eventos, ignora roots generados y nunca sigue symlinks de fuentes. Un rebuild fallido mantiene disponible el último sitio verificado mientras los diagnósticos siguen activos."),
            code_block(locale, "shell", "pliego dev 4400\npliego dev 4400 --lan\npliego dev 4400 --host 192.168.1.20"),
        ]),
        doc_section(locale, "hmr", "Typed HMR decisions", "Decisiones HMR tipadas", vec![
            definition_list(locale, &[
                ("css", "Refresh the affected stylesheet URL without replacing the document", "Actualiza la URL del stylesheet afectado sin reemplazar el documento"),
                ("content", "Fetch the rebuilt route after its verified artifact changes", "Obtiene la ruta recompilada después de cambiar su artefacto verificado"),
                ("adapter", "Retire the owned adapter generation before loading its replacement", "Retira la generación controlada del adapter antes de cargar su reemplazo"),
                ("reload", "Use a full document reload when the graph cannot prove a narrower action", "Usa reload completo cuando el grafo no puede probar una acción más específica"),
            ]),
            paragraph(locale, "HMR is derived from verified graph differences. It is an optimization of the development loop, never a second production runtime or an authority over unverified bytes.", "HMR se deriva de diferencias verificadas del grafo. Es una optimización del bucle de desarrollo, nunca un segundo runtime de producción ni una autoridad sobre bytes no verificados."),
        ]),
        doc_section(locale, "explain", "Explain causality", "Explica la causalidad", vec![
            code_block(locale, "shell", "pliego why artifact /\npliego why artifact assets/site.css\npliego why-rebuilt"),
            paragraph(locale, "why artifact verifies the current receipt and pliego.graph.json before tracing source-to-route-to-artifact edges. why-rebuilt reads the latest bounded local development record and reports changed sources, invalidated routes, affected artifacts, byte changes, HMR choice, and receipt transition.", "why artifact verifica el recibo actual y pliego.graph.json antes de seguir edges source-to-route-to-artifact. why-rebuilt lee el último registro local y limitado de desarrollo y reporta fuentes cambiadas, rutas invalidadas, artefactos afectados, cambios de bytes, decisión HMR y transición del recibo."),
        ]),
        doc_section(locale, "recover", "Failure preserves evidence", "El fallo preserva la evidencia", vec![
            paragraph(locale, "Compiler errors, invalid content, graph mismatches, and adapter build failures produce stable diagnostics without publishing a partial site. Correct the reported source and save again; the watcher retries from the last accepted generation.", "Los errores del compilador, contenido inválido, mismatches del grafo y fallos de build de adapters producen diagnósticos estables sin publicar un sitio parcial. Corrige la fuente reportada y guarda de nuevo; el watcher reintenta desde la última generación aceptada."),
            link_list(locale, &[
                ("/docs/errors-and-diagnostics", "Read diagnostic recovery", "Leer recuperación de diagnósticos"),
                ("/docs/artifact-trust", "Understand the verified graph", "Entender el grafo verificado"),
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

fn schemas_and_snapshots(locale: Locale) -> View {
    vec![
        doc_section(locale, "catalog", "Seal the event catalog", "Sella el catálogo de eventos", vec![
            paragraph(locale, "Each durable event kind has a portable name, a positive schema version, bounded canonical JSON, and a stable mapper identity. Sealing validates the complete catalog before any payload can enter a typed reducer.", "Cada tipo de evento durable tiene un nombre portable, una versión positiva de schema, JSON canónico limitado y una identidad estable de mapper. Sellar valida el catálogo completo antes de que cualquier payload pueda entrar en un reducer tipado."),
            definition_list(locale, &[
                ("kind", "Stable application-owned event identity", "Identidad estable de evento controlada por la aplicación"),
                ("version", "Exact schema version carried by the envelope", "Versión exacta del schema incluida en el envelope"),
                ("mapper", "Deterministic value-to-type admission identity", "Identidad determinista de admisión value-to-type"),
                ("limits", "Bounded kinds, payload bytes, depth, and remembered determinism pairs", "Tipos, bytes de payload, profundidad y pares de determinismo limitados"),
            ]),
        ]),
        doc_section(locale, "upcasting", "Upcast only adjacent versions", "Haz upcast sólo entre versiones adyacentes", vec![
            paragraph(locale, "A v1 payload reaches v3 only through explicit v1-to-v2 and v2-to-v3 edges. Gaps, cross-kind edges, duplicate steps, nondeterministic output, or an upcast beyond the current schema fail while the catalog is sealed.", "Un payload v1 llega a v3 sólo mediante edges explícitos v1-a-v2 y v2-a-v3. Gaps, edges entre tipos distintos, pasos duplicados, salida no determinista o un upcast posterior al schema actual fallan al sellar el catálogo."),
            code_block(locale, "text", "task.created/v1 --title-to-priority/1--> task.created/v2\ntask.created/v2 --priority-to-origin/1--> task.created/v3"),
            note(locale, "No implicit migration", "Changing a Rust struct does not rewrite durable history. Register and test each adjacent semantic transition before accepting old envelopes.", "Sin migración implícita", "Cambiar un struct Rust no reescribe historia durable. Registra y prueba cada transición semántica adyacente antes de aceptar envelopes antiguos."),
        ]),
        doc_section(locale, "identity", "Bind snapshot identity", "Vincula la identidad del snapshot", vec![
            paragraph(locale, "A projection snapshot binds the exact event content head, sealed schema-set digest, reducer identity, codec identity and configuration, canonical state bytes, and snapshot format. Its SHA-256 protects integrity; it does not establish remote authority.", "Un snapshot de proyección vincula el content head exacto de eventos, digest del conjunto de schemas sellado, identidad del reducer, identidad y configuración del codec, bytes canónicos del estado y formato del snapshot. Su SHA-256 protege integridad; no establece autoridad remota."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/30-event-schema-and-snapshot-contract.md", "Normative schema and snapshot contract", "Contrato normativo de schemas y snapshots")]),
        ]),
        doc_section(locale, "restore", "Restore fail closed", "Restaura de forma fail-closed", vec![
            steps(locale, &[
                ("Verify", "Decode bounded bytes and verify the snapshot digest and format", "Verificar", "Decodifica bytes limitados y verifica digest y formato del snapshot"),
                ("Match", "Require the exact current schema, reducer, codec, and content-head contract", "Comparar", "Exige el contrato actual exacto de schemas, reducer, codec y content head"),
                ("Replay", "Apply only the exact contiguous tail after the captured cursor", "Reproducir", "Aplica únicamente el tail contiguo exacto posterior al cursor capturado"),
                ("Compare", "Prove snapshot-tail and genesis replay produce identical canonical state", "Comparar", "Prueba que snapshot-tail y replay desde genesis producen estado canónico idéntico"),
            ]),
        ]),
    ].into_view()
}

fn hyphae_sync(locale: Locale) -> View {
    vec![
        doc_section(locale, "boundary", "Hyphae is optional durability", "Hyphae es durabilidad opcional", vec![
            paragraph(locale, "Static PliegoRS sites require no database. Applications that need shared durable history can use Hyphae through pliego-hyphae protocol v2. The crate defines and verifies the client boundary; it is not the Hyphae service implementation.", "Los sitios estáticos PliegoRS no requieren base de datos. Las aplicaciones que necesitan historia durable compartida pueden usar Hyphae mediante el protocolo v2 de pliego-hyphae. El crate define y verifica el límite cliente; no es la implementación del servicio Hyphae."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/29-hyphae-verified-sync-guide.md", "Read the normative sync guide", "Leer la guía normativa de sync")]),
        ]),
        doc_section(locale, "append", "Verify every append", "Verifica cada append", vec![
            paragraph(locale, "An append request binds stream identity, ordered local events, idempotency identity, and expected cursor. The response remains untrusted until its page attestation and every receipt signature resolve through the same accepted authority policy.", "Un request de append vincula identidad del stream, eventos locales ordenados, identidad de idempotencia y cursor esperado. La respuesta sigue sin confianza hasta que su page attestation y cada firma de recibo resuelven mediante la misma política de autoridad aceptada."),
            note(locale, "Shape is not trust", "Base64url syntax, UUIDv7 shape, timestamps, and key IDs can be validated without proving who signed the bytes. Only the configured verifier and authority policy cross that boundary.", "La forma no es confianza", "La sintaxis base64url, forma UUIDv7, timestamps y key IDs pueden validarse sin probar quién firmó los bytes. Sólo el verifier configurado y la política de autoridad cruzan ese límite."),
        ]),
        doc_section(locale, "pull", "Replay inside a fixed snapshot", "Reproduce dentro de un snapshot fijo", vec![
            paragraph(locale, "The first pull discovers a terminal snapshot cursor. Every continuation is bound to that exact head; pages cannot regress, fork at the same position, advance beyond the snapshot, change completion, or omit the attestation even when no events are returned.", "El primer pull descubre un cursor terminal de snapshot. Cada continuación queda vinculada a ese head exacto; las páginas no pueden retroceder, bifurcarse en la misma posición, avanzar más allá del snapshot, cambiar completion ni omitir la attestation aunque no retornen eventos."),
            code_block(locale, "text", "Latest(after) -> VerifiedPage(snapshot=S, next=A)\nExact(S, after=A) -> VerifiedPage(snapshot=S, next=S, complete=true)"),
        ]),
        doc_section(locale, "authority", "Consume verified state", "Consume estado verificado", vec![
            paragraph(locale, "Verification returns consuming typestate: unverified responses cannot expose admitted application events, and verified pages can be applied only against the stream, cursor, snapshot, and authority that produced them. Persisted evidence must be replayed through the same checks.", "La verificación retorna typestate consumible: las respuestas no verificadas no pueden exponer eventos admitidos de la aplicación y las páginas verificadas sólo pueden aplicarse contra el stream, cursor, snapshot y autoridad que las produjeron. La evidencia persistida debe reproducirse mediante los mismos checks."),
            link_list(locale, &[
                ("/docs/events-and-folds", "Model local events first", "Modelar primero eventos locales"),
                ("/docs/schemas-and-snapshots", "Bind schema and projection identity", "Vincular identidad de schema y proyección"),
            ]),
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

fn dom_lifecycle(locale: Locale) -> View {
    vec![
        doc_section(locale, "scopes", "One owner per mounted lifetime", "Un propietario por lifetime montado", vec![
            paragraph(locale, "A MountScope owns its exact DOM range, reactive descendants, listeners, observer handles, adapter generations, cancellation signals, and cleanup callbacks. Nested scopes form an explicit tree; disposing a parent retires the complete owned subtree.", "Un MountScope controla su rango DOM exacto, descendientes reactivos, listeners, handles de observers, generaciones de adapters, señales de cancelación y callbacks de cleanup. Los scopes anidados forman un árbol explícito; disponer un parent retira el subárbol controlado completo."),
            definition_list(locale, &[
                ("mount", "Claim detached authored nodes and install owned resources", "Reclama nodos de autoría detached e instala recursos controlados"),
                ("update", "Change only resources still owned by the live generation", "Cambia únicamente recursos aún controlados por la generación viva"),
                ("dispose", "Abort work, run LIFO cleanup, then remove exact owned nodes", "Aborta trabajo, ejecuta cleanup LIFO y luego elimina nodos controlados exactos"),
            ]),
        ]),
        doc_section(locale, "keyed", "Retain keyed identity", "Retén identidad keyed", vec![
            paragraph(locale, "Keyed reconciliation preserves existing node and listener identity, creates builders only for new keys, and minimizes browser moves. Duplicate keys, hostile topology, oversized updates, foreign gaps, unsupported parents, or moved descendants fail without claiming foreign DOM.", "La reconciliación keyed preserva identidad existente de nodos y listeners, crea builders sólo para keys nuevas y minimiza movimientos del navegador. Keys duplicadas, topología hostil, updates sobredimensionados, gaps externos, parents no soportados o descendientes movidos fallan sin reclamar DOM externo."),
        ]),
        doc_section(locale, "adoption", "Adopt exact SSR output", "Adopta salida SSR exacta", vec![
            paragraph(locale, "Adoptable rendering emits a versioned pliego:ssr:v1 seed. Browser preflight validates the complete structure, text, namespaces, attributes, dynamic first reads, and keyed identities before installing any listener or effect. A mismatch is diagnostic and non-mutating.", "El render adoptable emite un seed versionado pliego:ssr:v1. El preflight del navegador valida estructura completa, texto, namespaces, atributos, primeras lecturas dinámicas e identidades keyed antes de instalar listeners o efectos. Un mismatch es diagnóstico y no muta el documento."),
            note(locale, "Adoption is strict", "PliegoRS reuses exact authored nodes. It does not heuristically hydrate arbitrary third-party markup or silently replace a mismatched seed.", "La adopción es estricta", "PliegoRS reutiliza nodos exactos de autoría. No hidrata heurísticamente markup arbitrario de terceros ni reemplaza silenciosamente un seed incompatible."),
        ]),
        doc_section(locale, "cleanup", "Cleanup cannot be postponed", "El cleanup no puede posponerse", vec![
            paragraph(locale, "Scope disposal emits pliego:scope-dispose, aborts provisional plugin work, drains registered resources in LIFO order, and removes DOM last. A plugin promise that never settles cannot keep registered cleanup alive; a late result is retired without mounting the obsolete generation.", "Disponer el scope emite pliego:scope-dispose, aborta trabajo provisional de plugins, drena recursos registrados en orden LIFO y elimina el DOM al final. Una promesa de plugin que nunca termina no puede mantener vivo el cleanup registrado; un resultado tardío se retira sin montar la generación obsoleta."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/31-dom-lifecycle-contract.md", "Normative DOM lifecycle contract", "Contrato normativo del lifecycle DOM"),
                ("/docs/browser-runtime", "Adapter admission and policy", "Admisión y política de adapters"),
            ]),
        ]),
    ].into_view()
}

fn native_runtime(locale: Locale) -> View {
    vec![
        doc_section(locale, "status", "Evaluate G1 from main", "Evalúa G1 desde main", vec![
            paragraph(locale, "pliego-router and pliego-runtime 0.1.0-preview.1 are executable source work for G1. They are not published on crates.io, are not wired into the released 0.0.2 CLI, and do not promote the native HTTP or dynamic SSR capabilities from not-released.", "pliego-router y pliego-runtime 0.1.0-preview.1 son trabajo fuente ejecutable para G1. No están publicados en crates.io, no están conectados al CLI 0.0.2 liberado y no promueven las capacidades de HTTP nativo o SSR dinámico desde not-released."),
            code_block(locale, "shell", "git clone https://github.com/celiumsai/pliegors.git\ncd pliegors\ncargo test -p pliego-runtime -p native-pliego --locked\ncargo run -p native-pliego"),
            note(locale, "Preview boundary", "The reference application binds to 127.0.0.1:4310 by default. It is reproducible evidence, not a production server release or support promise.", "Límite del preview", "La aplicación de referencia escucha en 127.0.0.1:4310 de forma predeterminada. Es evidencia reproducible, no un release de servidor productivo ni una promesa de soporte."),
        ]),
        doc_section(locale, "ownership", "Keep transport and framework ownership explicit", "Mantén explícita la propiedad del transporte y del framework", vec![
            paragraph(locale, "The portable router seals route grammar, parameters, group and layout scopes, middleware capabilities, error-boundary identity, collisions, and a deterministic graph digest. The runtime owns admission, deadlines, cancellation, cleanup, response commitment, byte accounting, diagnostics, and receipts.", "El router portable sella gramática de rutas, parámetros, scopes de group y layout, capabilities de middleware, identidad de error boundaries, colisiones y un digest determinista del grafo. El runtime controla admisión, deadlines, cancelación, cleanup, commitment de respuesta, conteo de bytes, diagnósticos y recibos."),
            definition_list(locale, &[
                ("Axum / Hyper", "HTTP types, transport, and body framing", "Tipos HTTP, transporte y framing del body"),
                ("Tower", "Admission, load control, timeout, and middleware interoperability", "Admisión, control de carga, timeout e interoperabilidad de middleware"),
                ("Tokio", "Executor, timers, cancellation wakeups, and graceful shutdown", "Executor, timers, wakeups de cancelación y cierre ordenado"),
                ("PliegoRS", "Sealed application semantics, limits, failure rules, and evidence", "Semántica sellada de aplicación, límites, reglas de fallo y evidencia"),
            ]),
        ]),
        doc_section(locale, "rendering", "Choose one explicit SSR mode", "Elige un modo SSR explícito", vec![
            paragraph(locale, "Complete renders one bounded response before commitment. Ordered streams sibling factories under backpressure. Boundary starts a bounded set of futures concurrently, emits a stable inert data-pliego-boundary template anchor, and delivers useful resolved HTML in declaration order without a JavaScript patch runtime.", "Complete renderiza una respuesta limitada antes del commitment. Ordered transmite factories hermanas bajo backpressure. Boundary inicia de forma concurrente un conjunto limitado de futures, emite un anchor template data-pliego-boundary inerte y estable, y entrega HTML útil resuelto en orden de declaración sin un runtime JavaScript de parches."),
            code_block(locale, "rust", "use pliego_dom::{IntoView, el};\nuse pliego_runtime::{\n    AsyncBoundary, BoundaryDocument, BoundaryRenderOptions,\n    render_boundary_document,\n};\n\nlet document = BoundaryDocument::new(\"Account\");\nlet boundaries = [\n    AsyncBoundary::map(\"heading\", async { \"Account\" }, |title| {\n        el(\"h1\").child(title).into_view()\n    })?,\n];\nlet response = render_boundary_document(\n    &document,\n    boundaries,\n    BoundaryRenderOptions::default(),\n)?;"),
            note(locale, "Ordered semantics", "Boundary mode overlaps independent work but does not let later HTML overtake an earlier declaration. It is not React Flight, fallback replacement, or partial prerendering.", "Semántica ordenada", "El modo boundary solapa trabajo independiente, pero no permite que HTML posterior adelante una declaración anterior. No es React Flight, reemplazo de fallback ni partial prerendering."),
        ]),
        doc_section(locale, "failure", "Bound the work and preserve committed semantics", "Limita el trabajo y preserva la semántica ya comprometida", vec![
            paragraph(locale, "Boundary identities are validated and unique before commitment. Defaults allow 32 declarations, four in flight, and five seconds per future; hard ceilings are 256, 32, and 60 seconds. Shell, anchors, and resolved views share one output budget, while every view retains depth and node limits.", "Las identidades de boundary se validan y son únicas antes del commitment. Los valores predeterminados permiten 32 declaraciones, cuatro en vuelo y cinco segundos por future; los techos son 256, 32 y 60 segundos. Shell, anchors y vistas resueltas comparten un presupuesto de salida, mientras cada vista conserva límites de profundidad y nodos."),
            paragraph(locale, "A timeout, application failure, panic, or output exhaustion after commitment terminates the body and records failure; it cannot rewrite the status or emit a second page. PLG-REN-210 discards application error text at the boundary instead of admitting it into public diagnostics.", "Un timeout, fallo de aplicación, panic o agotamiento de salida después del commitment termina el body y registra el fallo; no puede reescribir el status ni emitir una segunda página. PLG-REN-210 descarta el texto del error de aplicación en el límite en vez de admitirlo en diagnósticos públicos."),
            note(locale, "Optional styling boundary", "PliegoCSS 0.1.0-rc.2 may be installed separately as an experimental build-time companion. Standard CSS remains the default; the native runtime does not link, install, or require PliegoCSS.", "Límite opcional de estilos", "PliegoCSS 0.1.0-rc.2 puede instalarse por separado como companion experimental de build. CSS estándar sigue siendo el valor predeterminado; el runtime nativo no enlaza, instala ni requiere PliegoCSS."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/49-native-runtime-preview.md", "Native runtime preview contract", "Contrato del preview del runtime nativo"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/evidence/g1-async-boundary-foundation.md", "Asynchronous boundary evidence", "Evidencia de boundaries asíncronos"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/rfc/RFC-008-native-runtime.md", "RFC-008 native runtime", "RFC-008 runtime nativo"),
            ]),
        ]),
    ].into_view()
}

fn opensdk(locale: Locale) -> View {
    vec![
        doc_section(locale, "status", "Evaluate the preview from main", "Evalúa el preview desde main", vec![
            paragraph(locale, "OpenSDK 0.1.0-preview.1 landed after PliegoRS v0.0.2. Its implementation and conformance suites are available in the repository, but pliego-sdk is not published on crates.io and the 0.0.2 CLI does not contain this command surface.", "OpenSDK 0.1.0-preview.1 llegó después de PliegoRS v0.0.2. Su implementación y suites de conformidad están disponibles en el repositorio, pero pliego-sdk no está publicado en crates.io y el CLI 0.0.2 no contiene esta superficie de comandos."),
            code_block(locale, "shell", "git clone https://github.com/celiumsai/pliegors.git\ncd pliegors\ncargo run -p pliego-cli --locked -- sdk compatibility\nnpm ci\nnpm run check:opensdk:all"),
            note(locale, "Toolchain boundary", "The current main branch requires Rust 1.86.0 for the Wasmtime 36.0.8 security floor. Released v0.0.2 artifacts retain their original Rust 1.85.0 evidence.", "Límite del toolchain", "El branch main actual requiere Rust 1.86.0 por el security floor de Wasmtime 36.0.8. Los artefactos v0.0.2 liberados conservan su evidencia original de Rust 1.85.0."),
        ]),
        doc_section(locale, "admission", "Admit exact bytes before execution", "Admite bytes exactos antes de ejecutar", vec![
            paragraph(locale, "An extension manifest declares its identity, exact entry points, OpenSDK API, compatible host range, required features, requested capabilities, and resource budgets. Admission verifies the component digest and policy before returning a ValidatedExtension typestate that a runtime can execute.", "El manifest de una extensión declara identidad, entry points exactos, API OpenSDK, rango de host compatible, features requeridos, capabilities solicitadas y presupuestos de recursos. La admisión verifica el digest del componente y la política antes de retornar el typestate ValidatedExtension que un runtime puede ejecutar."),
            code_block(locale, "rust", "use pliego_sdk::{CapabilityPolicy, ExtensionManifest, HostContract};\nuse semver::Version;\n\nlet manifest: ExtensionManifest = serde_json::from_slice(manifest_bytes)?;\nlet host = HostContract::preview(\n    Version::parse(\"0.1.0-preview.1\")?,\n    CapabilityPolicy::deny_all(),\n);\nlet validated = host.admit(manifest, component_bytes)?;"),
            note(locale, "A request is not authority", "Declaring a capability never creates a filesystem, network, clock, random, environment, or HTTP handle. The host policy and application-supplied executor remain the resource boundary.", "Una solicitud no es autoridad", "Declarar una capability nunca crea un handle de filesystem, red, reloj, random, environment o HTTP. La política del host y el executor suministrado por la aplicación siguen siendo el límite del recurso."),
        ]),
        doc_section(locale, "effects", "Broker effects and retain evidence", "Intermedia efectos y conserva evidencia", vec![
            paragraph(locale, "Non-deterministic work crosses EffectBroker only after both the broker and specific effect capabilities pass policy. Calls that reach the executor produce ordered SHA-256 receipts for success or error. Denials happen before execution and do not claim that an effect occurred.", "El trabajo no determinista cruza EffectBroker sólo después de que la capability del broker y la del efecto específico superan la política. Las llamadas que alcanzan el executor producen recibos SHA-256 ordenados para success o error. Las denegaciones suceden antes de ejecutar y no afirman que ocurrió un efecto."),
            definition_list(locale, &[
                ("Fuel and deadline", "Bound computation and interrupt an expired invocation", "Limitan el cómputo e interrumpen una invocación vencida"),
                ("Memory and output", "Reject growth or results beyond the declared budgets", "Rechazan crecimiento o resultados fuera de los presupuestos declarados"),
                ("Effect receipts", "Bind ordered operation, request, outcome, and response evidence", "Vinculan operación, solicitud, resultado y evidencia de respuesta en orden"),
                ("Ambient WASI", "Not linked by the reference component host", "No está enlazado por el host de componentes de referencia"),
            ]),
        ]),
        doc_section(locale, "governance", "Implemented does not mean stable", "Implementado no significa estable", vec![
            paragraph(locale, "RFC-006 and RFC-007 remain Draft, and ADR-006 remains Proposed. The implementation is executable evidence for governance and external evaluation; incompatible changes may arrive in another preview protocol. None of this surface is labeled stable.", "RFC-006 y RFC-007 siguen Draft, y ADR-006 sigue Proposed. La implementación es evidencia ejecutable para gobernanza y evaluación externa; pueden llegar cambios incompatibles en otro protocolo preview. Ninguna parte de esta superficie se etiqueta como estable."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/42-opensdk-foundation.md", "OpenSDK foundation", "Fundamentos de OpenSDK"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/rfc/RFC-006-opensdk-planes-and-capabilities.md", "RFC-006: planes and capabilities", "RFC-006: planos y capabilities"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/rfc/RFC-007-pliego-build-output-contract.md", "RFC-007: portable build output", "RFC-007: salida de build portable"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/adr/ADR-006-opensdk-wasmtime-security-floor.md", "ADR-006: Wasmtime security floor", "ADR-006: security floor de Wasmtime"),
            ]),
        ]),
    ].into_view()
}

fn opensdk_components(locale: Locale) -> View {
    vec![
        doc_section(locale, "component", "Typed Wasm Component execution", "Ejecución tipada de Wasm Components", vec![
            paragraph(locale, "The reference sandbox compiles and admits a Rust WebAssembly Component against pliego:build/transformer@0.1.0. The host instantiates it without ambient WASI, invokes the typed export, and emits a schema-valid build transform receipt over framed input and output digests.", "El sandbox de referencia compila y admite un WebAssembly Component Rust contra pliego:build/transformer@0.1.0. El host lo instancia sin WASI ambiental, invoca el export tipado y emite un recibo de transformación válido contra schema sobre digests enmarcados de entrada y salida."),
            note(locale, "Preview protocol", "This conformance surface belongs to OpenSDK 0.1.0-preview.1 on main and is not part of the published PliegoRS v0.0.2 packages.", "Protocolo preview", "Esta superficie de conformidad pertenece a OpenSDK 0.1.0-preview.1 en main y no forma parte de los paquetes PliegoRS v0.0.2 publicados."),
            code_block(locale, "shell", "cargo run -p pliego-cli --locked -- sdk test \\\n  fixtures/opensdk/rust-component/pliego-extension.json \\\n  --input fixtures/opensdk/transform-input.json \\\n  --format json"),
        ]),
        doc_section(locale, "languages", "One contract, independent implementations", "Un contrato, implementaciones independientes", vec![
            paragraph(locale, "The conformance fixture implements the same bounded build transform in Rust, TypeScript, and Python process bridges plus the Rust Wasm Component. The gate runs each implementation twice and requires exact canonical result bytes across all four.", "El fixture de conformidad implementa la misma transformación limitada en bridges de proceso Rust, TypeScript y Python, además del Wasm Component Rust. El gate ejecuta cada implementación dos veces y exige bytes canónicos exactos en las cuatro."),
            code_block(locale, "shell", "npm run check:opensdk:multilang"),
            definition_list(locale, &[
                ("Rust Component", "Reference sandboxed Component Model implementation", "Implementación sandboxed de referencia para Component Model"),
                ("TypeScript", "Conformance process bridge transpiled with esbuild and run on Node.js 20", "Bridge de proceso de conformidad transpilado con esbuild y ejecutado en Node.js 20"),
                ("Python", "Independent conformance process bridge", "Bridge de proceso de conformidad independiente"),
            ]),
        ]),
        doc_section(locale, "budgets", "Prove resource failures", "Prueba fallos de recursos", vec![
            paragraph(locale, "The component gate also proves finite fuel exhaustion, wall-time interruption, memory rejection, output rejection, and receipt validation. Instantiation and invocation share one deadline, and each execution receives an isolated Wasmtime engine.", "El gate del componente también prueba agotamiento de fuel finito, interrupción por wall-time, rechazo de memoria, rechazo de output y validación del recibo. Instanciación e invocación comparten un deadline y cada ejecución recibe un engine Wasmtime aislado."),
        ]),
        doc_section(locale, "limits", "Know what the evidence means", "Entiende qué significa la evidencia", vec![
            paragraph(locale, "This fixture proves protocol equivalence for one build transform. The TypeScript and Python bridges are native-trusted conformance implementations, not sandboxed Component Model SDKs. A third stable Component SDK still requires hosted cross-platform Component Model evidence.", "Este fixture prueba equivalencia de protocolo para una transformación de build. Los bridges TypeScript y Python son implementaciones de conformidad native-trusted, no SDKs sandboxed de Component Model. Un tercer SDK Component estable aún requiere evidencia hosted y cross-platform de Component Model."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/43-opensdk-multilang-conformance.md", "Multilanguage evidence contract", "Contrato de evidencia multilenguaje"),
                ("/docs/opensdk-compatibility", "Read the support matrix boundary", "Leer el límite de la matriz de soporte"),
            ]),
        ]),
    ].into_view()
}

fn browser_framework_conformance(locale: Locale) -> View {
    vec![
        doc_section(locale, "native", "Keep framework runtimes native", "Mantén nativos los runtimes", vec![
            paragraph(locale, "React, Svelte, and Lit keep their normal browser packages and APIs. PliegoRS does not reimplement their renderers. Each fixture is exposed through a Custom Element and admitted through adapter API v1, leaving the real integration point visible to developers.", "React, Svelte y Lit conservan sus paquetes y APIs normales del navegador. PliegoRS no reimplementa sus renderers. Cada fixture se expone mediante un Custom Element y se admite mediante adapter API v1, dejando visible para el desarrollador el punto real de integración."),
            note(locale, "Preview protocol", "These fixtures exercise OpenSDK 0.1.0-preview.1 from main; they are conformance evidence rather than a released browser framework SDK.", "Protocolo preview", "Estos fixtures ejercitan OpenSDK 0.1.0-preview.1 desde main; son evidencia de conformidad, no un SDK de frameworks web liberado."),
            note(locale, "Conformance, not a new abstraction", "The wrappers are examples and test inputs. They do not create another component model over React, Svelte, or Lit.", "Conformidad, no una abstracción nueva", "Los wrappers son ejemplos y entradas de prueba. No crean otro modelo de componentes sobre React, Svelte o Lit."),
        ]),
        doc_section(locale, "descriptor", "Match the admitted descriptor", "Coincide con el descriptor admitido", vec![
            paragraph(locale, "Before registration, the exported component tag, adapter API version, lifecycle exports, and capabilities must match the exact admitted manifest descriptor. Reserved or malformed Custom Element names and incoherent lifecycle combinations are rejected.", "Antes del registro, el tag exportado del componente, versión de API del adapter, exports de lifecycle y capabilities deben coincidir con el descriptor exacto admitido en el manifest. Se rechazan nombres reservados o inválidos de Custom Elements y combinaciones incoherentes de lifecycle."),
            definition_list(locale, &[
                ("mount", "Create one owned framework root", "Crea un root controlado del framework"),
                ("update", "Apply live properties to the admitted generation", "Aplica propiedades en vivo a la generación admitida"),
                ("HMR", "Dispose the old root before importing and mounting a fresh generation", "Dispone el root anterior antes de importar y montar una generación nueva"),
                ("destroy", "Retire roots, listeners, timers, channels, and adapter scopes", "Retira roots, listeners, timers, channels y scopes del adapter"),
            ]),
        ]),
        doc_section(locale, "gate", "Run real browser conformance", "Ejecuta conformidad real en navegador", vec![
            code_block(locale, "shell", "npm run check:opensdk:browser-frameworks"),
            paragraph(locale, "The headless Chrome gate covers mount, live property update, reduced-motion propagation, HMR replacement, runtime destruction, and listener cleanup for React, Svelte, and Lit.", "El gate de Chrome headless cubre mount, actualización de propiedades, propagación de reduced motion, reemplazo HMR, destrucción del runtime y cleanup de listeners para React, Svelte y Lit."),
        ]),
        doc_section(locale, "cleanup-evidence", "Measure cleanup under adversarial ownership", "Mide cleanup bajo ownership adversarial", vec![
            paragraph(locale, "A separate adversarial adapter owns a real interval, document listener, abort scope, and MessageChannel. HMR must replace them without duplication; destroy must reduce all four counters to zero, and later events or time may not change the retired counter stream.", "Un adapter adversarial separado controla un interval real, listener del documento, abort scope y MessageChannel. HMR debe reemplazarlos sin duplicación; destroy debe reducir los cuatro contadores a cero y los eventos o el tiempo posteriores no pueden cambiar el stream de contadores retirado."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/44-browser-framework-conformance.md", "Browser conformance evidence", "Evidencia de conformidad web"),
                ("/docs/browser-runtime", "Understand adapter admission", "Entender la admisión de adapters"),
                ("/docs/dom-lifecycle", "Understand deterministic cleanup", "Entender el cleanup determinista"),
            ]),
        ]),
    ].into_view()
}

fn opensdk_tooling(locale: Locale) -> View {
    vec![
        doc_section(locale, "protocol", "One bounded tooling plane", "Un plano de tooling limitado", vec![
            paragraph(locale, "The OpenSDK tooling host uses newline-delimited JSON-RPC 2.0 over standard input and output. Editors negotiate through pliego/handshake; the reference MCP adapter exposes the same capability handshake after completing the MCP 2025-11-25 initialization lifecycle.", "El host de tooling OpenSDK usa JSON-RPC 2.0 delimitado por líneas sobre entrada y salida estándar. Los editores negocian mediante pliego/handshake; el adapter MCP de referencia expone el mismo handshake de capabilities después de completar el lifecycle de inicialización MCP 2025-11-25."),
            definition_list(locale, &[
                ("OpenSDK", "0.1.0-preview.1", "0.1.0-preview.1"),
                ("JSON-RPC", "2.0 over bounded newline-delimited messages", "2.0 sobre mensajes delimitados por líneas y limitados"),
                ("MCP", "2025-11-25 reference client surface", "Superficie del cliente de referencia 2025-11-25"),
            ]),
        ]),
        doc_section(locale, "editor", "Negotiate before diagnostics", "Negocia antes de diagnósticos", vec![
            code_block(locale, "shell", "cargo run -p pliego-cli --locked -- sdk tooling-host \\\n  --protocol pliego \\\n  --feature diagnostic-links"),
            paragraph(locale, "The native handshake must include protocolVersion 0.1.0-preview.1. Missing or incompatible versions fail before any diagnostic or project method becomes available.", "El handshake nativo debe incluir protocolVersion 0.1.0-preview.1. Las versiones ausentes o incompatibles fallan antes de que esté disponible cualquier método de diagnóstico o proyecto."),
        ]),
        doc_section(locale, "mcp", "MCP is a client, not a privilege", "MCP es un cliente, no un privilegio", vec![
            code_block(locale, "shell", "cargo run -p pliego-cli --locked -- sdk tooling-host \\\n  --protocol mcp \\\n  --feature diagnostic-links\nnpm run check:opensdk:tooling"),
            paragraph(locale, "The MCP client receives no implicit filesystem root, network access, sampling, elicitation, or project capability. Its reference tool accepts only the empty arguments object declared by its schema and cannot bypass OpenSDK admission policy.", "El cliente MCP no recibe filesystem root, acceso de red, sampling, elicitation ni capability de proyecto implícitos. Su tool de referencia acepta únicamente el objeto vacío declarado por su schema y no puede evadir la política de admisión OpenSDK."),
        ]),
        doc_section(locale, "limits", "Fail with structured boundaries", "Falla con límites estructurados", vec![
            paragraph(locale, "The host accepts at most 10,000 requests per process and 1 MiB per JSON line. Invalid JSON-RPC versions, unknown fields, unsupported MCP versions, pre-initialization calls, unknown methods, and unknown tools return structured errors; notifications produce no response.", "El host acepta como máximo 10.000 requests por proceso y 1 MiB por línea JSON. Versiones JSON-RPC inválidas, campos desconocidos, versiones MCP incompatibles, llamadas previas a initialization, métodos desconocidos y tools desconocidos retornan errores estructurados; las notifications no producen respuesta."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/45-opensdk-tooling-protocol.md", "Complete tooling protocol", "Protocolo completo de tooling")]),
        ]),
    ].into_view()
}

fn opensdk_compatibility(locale: Locale) -> View {
    vec![
        doc_section(locale, "matrix", "Read the machine-verifiable matrix", "Lee la matriz verificable por máquinas", vec![
            code_block(locale, "shell", "cargo run -p pliego-cli --locked -- sdk compatibility --format json"),
            paragraph(locale, "The output conforms to pliego.sdk-compatibility-matrix.schema.json and records tested hosts, language toolchains, browser surfaces, tooling protocols, and deprecations. An integration is supported only at the boundary and version listed by that matrix.", "La salida cumple pliego.sdk-compatibility-matrix.schema.json y registra hosts probados, toolchains de lenguajes, superficies web, protocolos de tooling y deprecaciones. Una integración tiene soporte únicamente en el límite y versión incluidos por esa matriz."),
        ]),
        doc_section(locale, "stability", "Distinguish stability tiers", "Distingue niveles de estabilidad", vec![
            definition_list(locale, &[
                ("experimental", "May change without a compatibility window and is never enabled implicitly", "Puede cambiar sin ventana de compatibilidad y nunca se habilita implícitamente"),
                ("preview", "Versioned and tested, with breaking changes allowed in a new preview protocol", "Versionado y probado, con cambios incompatibles permitidos en un nuevo protocolo preview"),
                ("stable", "Preserves the contract for the documented compatibility window", "Preserva el contrato durante la ventana de compatibilidad documentada"),
            ]),
            note(locale, "Current tier", "OpenSDK 0.1.0-preview.1 is preview. Rust is the reference Component Model toolchain; TypeScript and Python are process-bridge conformance implementations.", "Nivel actual", "OpenSDK 0.1.0-preview.1 es preview. Rust es el toolchain Component Model de referencia; TypeScript y Python son implementaciones de conformidad mediante bridges de proceso."),
        ]),
        doc_section(locale, "deprecation", "Require an executable migration path", "Exige una ruta de migración ejecutable", vec![
            steps(locale, &[
                ("Publish", "Record the deprecated contract and its replacement in the matrix", "Publicar", "Registra en la matriz el contrato deprecado y su reemplazo"),
                ("Prove", "Give the replacement equivalent conformance coverage", "Probar", "Da al reemplazo cobertura de conformidad equivalente"),
                ("Wait", "Keep at least one later OpenSDK protocol version before removal", "Esperar", "Conserva al menos una versión posterior del protocolo OpenSDK antes de eliminar"),
                ("Migrate", "Publish documentation and a diagnostic that identifies the replacement", "Migrar", "Publica documentación y un diagnóstico que identifique el reemplazo"),
            ]),
            paragraph(locale, "Security fixes may disable unsafe behavior immediately, while the contract identifier remains visible with a security note until the next protocol release.", "Los fixes de seguridad pueden deshabilitar comportamiento inseguro inmediatamente, mientras el identificador del contrato permanece visible con una nota de seguridad hasta el siguiente release del protocolo."),
        ]),
        doc_section(locale, "source", "Keep the contract provider-neutral", "Mantén el contrato independiente del proveedor", vec![
            paragraph(locale, "The public compatibility authority is the celiumsai/pliegors repository: schemas, WIT packages, Rust types, fixtures, and conformance reports. A deployment product may consume those contracts, but no private service is required to implement or validate OpenSDK.", "La autoridad pública de compatibilidad es el repositorio celiumsai/pliegors: schemas, paquetes WIT, tipos Rust, fixtures y reportes de conformidad. Un producto de despliegue puede consumir esos contratos, pero ningún servicio privado es necesario para implementar o validar OpenSDK."),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/46-opensdk-compatibility-and-deprecation.md", "Compatibility and deprecation contract", "Contrato de compatibilidad y deprecación"),
                ("https://github.com/celiumsai/pliegors/blob/main/schemas/pliego.sdk-compatibility-matrix.schema.json", "Compatibility matrix schema", "Schema de la matriz de compatibilidad"),
                ("/docs/opensdk", "Return to the OpenSDK boundary", "Volver al límite OpenSDK"),
            ]),
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

fn artifact_trust(locale: Locale) -> View {
    vec![
        doc_section(locale, "namespace", "One portable output namespace", "Un namespace portable de salida", vec![
            paragraph(locale, "Routes, redirects, public assets, generated client files, the causal graph, and the build receipt share one collision model. Parent traversal, aliases, case-only collisions, Windows reserved names, symlinks, hardlinks, non-regular files, and output paths that overlap source are rejected before publication.", "Rutas, redirects, assets públicos, archivos generados del cliente, grafo causal y recibo del build comparten un modelo de colisiones. Parent traversal, aliases, colisiones sólo por casing, nombres reservados de Windows, symlinks, hardlinks, archivos no regulares y rutas de salida que se solapan con fuentes se rechazan antes de publicar."),
        ]),
        doc_section(locale, "capture", "Capture exact inputs", "Captura entradas exactas", vec![
            paragraph(locale, "The build captures portable source identities, sizes, SHA-256 digests, project configuration, toolchain identity, source revision, and producer declarations. Inputs are revalidated before publication so a file cannot change between planning and commit without invalidating the build.", "El build captura identidades portables de fuentes, tamaños, digests SHA-256, configuración del proyecto, identidad del toolchain, revisión fuente y declaraciones del productor. Las entradas se revalidan antes de publicar para que un archivo no pueda cambiar entre planificación y commit sin invalidar el build."),
            note(locale, "Environment is not invisible", "A reproducible producer must declare every environment value that changes output. Builder-only variables cannot leak into consumer or release identity.", "El environment no es invisible", "Un productor reproducible debe declarar cada valor de environment que cambie la salida. Variables exclusivas del builder no pueden filtrarse hacia la identidad del consumidor o del release."),
        ]),
        doc_section(locale, "receipt", "Verify receipt and graph together", "Verifica recibo y grafo juntos", vec![
            definition_list(locale, &[
                ("pliego.build.json", "Exact output file set, byte size, digest, producer, source revision, and toolchain", "Conjunto exacto de archivos, tamaño, digest, productor, revisión fuente y toolchain"),
                ("pliego.graph.json", "Versioned source-to-route-to-artifact causal edges bound by the receipt", "Edges causales versionados source-to-route-to-artifact vinculados por el recibo"),
                ("pliego inspect", "Recompute and verify the complete published artifact", "Recalcula y verifica el artefacto publicado completo"),
                ("pliego why artifact", "Explain only after receipt and graph verification succeed", "Explica sólo después de verificar recibo y grafo"),
            ]),
        ]),
        doc_section(locale, "publish", "Stage, seal, replace", "Stage, sella, reemplaza", vec![
            steps(locale, &[
                ("Preflight", "Validate budgets, namespace, inputs, and existing output without following links", "Preflight", "Valida budgets, namespace, entradas y salida existente sin seguir links"),
                ("Stage", "Write every new file into a private sibling directory", "Stage", "Escribe cada archivo nuevo en un directorio privado adyacente"),
                ("Seal", "Revalidate inputs and write the final receipt over the exact staged bytes", "Sellar", "Revalida entradas y escribe el recibo final sobre los bytes exactos del stage"),
                ("Replace", "Atomically swap the verified stage while retaining recoverability on failure", "Reemplazar", "Intercambia atómicamente el stage verificado preservando recuperación ante fallo"),
            ]),
            link_list(locale, &[("/docs/build-and-deploy", "Build, release, and deploy", "Build, release y despliegue")]),
        ]),
    ].into_view()
}

fn release_trust(locale: Locale) -> View {
    vec![
        doc_section(locale, "bytes", "Start with the exact release bytes", "Comienza por los bytes exactos del release", vec![
            paragraph(locale, "A release is an exact set, not a download page. PliegoRS publishes deterministic platform archives, a deterministic source archive, SHA-256 checksums, a signed manifest, offline verifiers, and bounded reproduction instructions. Missing and extra files are failures.", "Un release es un conjunto exacto, no una página de descargas. PliegoRS publica archives deterministas por plataforma, un archive fuente determinista, checksums SHA-256, un manifest firmado, verificadores offline e instrucciones limitadas de reproducción. Los archivos faltantes y adicionales son fallos."),
            code_block(locale, "shell", "node verify-release-bundle.mjs --dir ."),
        ]),
        doc_section(locale, "signatures", "Separate continuity from hosted identity", "Separa continuidad de identidad alojada", vec![
            definition_list(locale, &[
                ("Ed25519", "Project-controlled continuity signature over the exact primary release set", "Firma de continuidad controlada por el proyecto sobre el conjunto primario exacto del release"),
                ("Sigstore", "Keyless GitHub OIDC evidence for attestations and golden-matrix output", "Evidencia keyless de GitHub OIDC para attestations y salida de la matriz golden"),
                ("SHA-256", "Content identity checked before any installer or runner executes", "Identidad de contenido verificada antes de ejecutar cualquier instalador o runner"),
            ]),
            note(locale, "Independent mechanisms", "The hosted identity does not replace the project continuity key, and neither signature makes unreviewed source trustworthy by itself.", "Mecanismos independientes", "La identidad alojada no reemplaza la clave de continuidad del proyecto y ninguna firma vuelve confiable por sí sola una fuente no revisada."),
        ]),
        doc_section(locale, "attestations", "Inspect SBOM and provenance", "Inspecciona SBOM y provenance", vec![
            paragraph(locale, "Every promoted candidate carries a normalized CycloneDX SBOM and an in-toto Statement using the SLSA provenance v1 predicate. The offline verifier binds both documents to the exact release manifest and rejects substitution, drift, missing subjects, and extra package files.", "Cada candidato promovido contiene un SBOM CycloneDX normalizado y un Statement in-toto que usa el predicate SLSA provenance v1. El verificador offline vincula ambos documentos al manifest exacto del release y rechaza sustitución, drift, subjects faltantes y archivos adicionales."),
            note(locale, "Claim boundary", "A SLSA-compatible statement is not a claimed SLSA build level. A level requires its complete hosted, isolation, distribution, and independent-verification requirements.", "Límite de afirmación", "Un statement compatible con SLSA no equivale a afirmar un nivel SLSA. Un nivel exige sus requisitos completos de hosting, aislamiento, distribución y verificación independiente."),
        ]),
        doc_section(locale, "promotion", "Promote only release evidence", "Promueve sólo evidencia del release", vec![
            steps(locale, &[
                ("Candidate", "Build the signed bytes twice and exercise the signed runner on eight clean hosted environments.", "Candidato", "Construye dos veces los bytes firmados y ejercita el runner firmado en ocho entornos alojados limpios."),
                ("Registry", "Publish the exact crate graph from the same clean revision, then exercise WSL2 against those registry packages.", "Registry", "Publica el grafo exacto de crates desde la misma revisión limpia y luego ejercita WSL2 contra esos paquetes del registry."),
                ("Draft", "Rebuild the same revision, require one release-manifest digest across nine environments, and create a reviewable draft.", "Draft", "Recompila la misma revisión, exige un digest del manifest de release en nueve entornos y crea un draft revisable."),
                ("Release", "Publish only after the exact-set, attestation, matrix, and operator review gates agree.", "Release", "Publica sólo cuando coincidan los gates de conjunto exacto, attestations, matriz y revisión del operador."),
            ]),
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/37-supply-chain-attestations.md", "Supply-chain attestation contract", "Contrato de attestations de supply chain"),
                ("https://github.com/celiumsai/pliegors/blob/main/docs/40-release-only-golden-matrix.md", "Release-only golden matrix", "Matriz golden exclusiva del release"),
            ]),
        ]),
    ].into_view()
}

fn performance_evidence(locale: Locale) -> View {
    vec![
        doc_section(locale, "protocol", "Measure a declared experiment", "Mide un experimento declarado", vec![
            paragraph(locale, "PliegoRS retains raw build and browser observations together with the exact revision, operating system, CPU, memory, storage declaration, Rust, Node, browser, sample count, cache policy, and known uncontrolled variables. p50 and p95 use nearest-rank without dropping outliers.", "PliegoRS conserva observaciones crudas de build y navegador junto con la revisión exacta, sistema operativo, CPU, memoria, storage declarado, Rust, Node, navegador, cantidad de muestras, política de cache y variables no controladas conocidas. p50 y p95 usan nearest-rank sin descartar outliers."),
            definition_list(locale, &[
                ("Build", "Clean cold, no-change warm, content-only, CSS-only, and Rust-view observations", "Observaciones cold limpio, warm sin cambios, sólo contenido, sólo CSS y vista Rust"),
                ("Browser", "Signal updates, final DOM state, WASM linear memory, and mount/dispose residue", "Updates de signals, estado DOM final, memoria lineal WASM y residuo de mount/dispose"),
            ]),
        ]),
        doc_section(locale, "reproduce", "Reproduce before comparing", "Reproduce antes de comparar", vec![
            code_block(locale, "shell", "node scripts/measure-p8-builds.mjs\nsh scripts/build-browser-benchmark.sh\nnode scripts/measure-browser-benchmark.mjs\nnode scripts/merge-p8-benchmark-report.mjs\nnpm run check:benchmarks"),
            paragraph(locale, "The merger requires both sections to name the same clean commit. A dirty smoke run may test the harness, but it is rejected as publishable evidence.", "El merger exige que ambas secciones nombren el mismo commit limpio. Un smoke run dirty puede probar el harness, pero se rechaza como evidencia publicable."),
        ]),
        doc_section(locale, "adversarial", "Pair speed with failure evidence", "Combina velocidad con evidencia de fallos", vec![
            paragraph(locale, "Performance does not excuse unsafe parsing or unbounded work. Fuzz targets and adversarial suites exercise manifests, receipts, graphs, paths, release bundles, telemetry state, content limits, and state restoration independently from benchmark timing.", "El rendimiento no excusa parsing inseguro ni trabajo sin límites. Los fuzz targets y suites adversariales ejercitan manifests, receipts, grafos, paths, bundles de release, estado de telemetría, límites de contenido y restauración de estado de forma independiente al timing de benchmarks."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/38-fuzzing-and-adversarial-testing.md", "Fuzzing and adversarial testing", "Fuzzing y pruebas adversariales")]),
        ]),
        doc_section(locale, "interpret", "Keep observations inside their boundary", "Mantén las observaciones dentro de su límite", vec![
            note(locale, "No universal benchmark claim", "A result describes one revision and environment. It is not a guarantee, a device budget, a competitor comparison, or evidence for another commit.", "Sin afirmación universal", "Un resultado describe una revisión y un entorno. No es una garantía, un presupuesto de dispositivo, una comparación con competidores ni evidencia para otro commit."),
            paragraph(locale, "The published P8 local baseline is useful for regression detection because it preserves its raw samples and limitations. Hosted candidate evidence remains a separate release gate.", "El baseline local publicado de P8 sirve para detectar regresiones porque conserva sus muestras crudas y limitaciones. La evidencia del candidato alojado sigue siendo un gate de release separado."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/39-reproducible-benchmarks.md", "Benchmark protocol and current baseline", "Protocolo de benchmarks y baseline actual")]),
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

fn telemetry(locale: Locale) -> View {
    vec![
        doc_section(locale, "default", "Disabled means absent", "Desactivada significa ausente", vec![
            paragraph(locale, "PliegoRS telemetry is disabled by default. Installation, scaffolding, checking, development, and building do not create telemetry state or make telemetry network requests. No environment variable, manifest, installer flag, or account can silently enable it.", "La telemetría de PliegoRS está desactivada por defecto. La instalación, scaffold, verificación, desarrollo y build no crean estado de telemetría ni realizan requests de telemetría. Ninguna variable de environment, manifest, flag del instalador o cuenta puede activarla silenciosamente."),
            code_block(locale, "shell", "pliego telemetry status\npliego telemetry enable"),
        ]),
        doc_section(locale, "allowlist", "Exact bounded allowlist", "Allowlist exacta y limitada", vec![
            paragraph(locale, "An enabled journal retains at most 64 events. Every event contains only a local sequence, one of install/new/check/dev/build, coarse Unix day, CLI version, operating-system platform, and CPU architecture.", "Un journal habilitado conserva como máximo 64 eventos. Cada evento contiene sólo una secuencia local, uno de install/new/check/dev/build, día Unix aproximado, versión del CLI, plataforma del sistema operativo y arquitectura de CPU."),
            note(locale, "What is excluded", "There is no user or installation ID, IP, timestamp, path, project, template, route, command argument, source, environment value, error, dependency, hostname, username, or email.", "Lo que se excluye", "No hay ID de usuario o instalación, IP, timestamp, path, proyecto, template, route, argumento, source, valor de environment, error, dependencia, hostname, username ni email."),
        ]),
        doc_section(locale, "control", "Preview and export locally", "Previsualiza y exporta localmente", vec![
            code_block(locale, "shell", "pliego telemetry preview\npliego telemetry preview --format json\npliego telemetry export --output ./pliegors-telemetry.json"),
            paragraph(locale, "Preview shows the exact report shape. Export creates a new local file and refuses to overwrite an existing path. Neither command uploads data.", "Preview muestra la forma exacta del reporte. Export crea un archivo local nuevo y se niega a sobrescribir un path existente. Ningún comando sube datos."),
        ]),
        doc_section(locale, "delete", "Stop and delete under user control", "Detén y elimina bajo control del usuario", vec![
            code_block(locale, "shell", "pliego telemetry disable\npliego telemetry disable --delete-local"),
            paragraph(locale, "Disabling stops collection; --delete-local also removes the bounded local state. Re-enabling requires another deliberate command. Corrupt or unsupported state fails closed and cannot enable collection.", "Desactivar detiene la recolección; --delete-local también elimina el estado local limitado. Reactivar exige otro comando deliberado. Un estado corrupto o incompatible falla cerrado y no puede habilitar la recolección."),
            note(locale, "No network collector", "P8 has no submit command, collector URL, API key, cookie, retry queue, or background process. A future collector requires a separate policy and cannot remotely activate existing clients.", "Sin collector de red", "P8 no tiene comando submit, URL de collector, API key, cookie, cola de reintentos ni proceso en background. Un collector futuro exige otra política y no puede activar remotamente clientes existentes."),
            link_list(locale, &[("https://github.com/celiumsai/pliegors/blob/main/docs/41-voluntary-telemetry.md", "Complete telemetry contract", "Contrato completo de telemetría")]),
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
            code_block(locale, "shell", "# Run only after downloading the installer to disk\n./install.sh --version 0.0.2\n\n# Explicit mutable-channel opt-in\n./install.sh --channel latest"),
        ]),
        doc_section(locale, "deploy", "Deploy the static output", "Despliega la salida estática", vec![
            paragraph(locale, "Deploy the contents of target/site to any origin that preserves paths, MIME types, immutable asset caching, the authored 404 document, and clean-route fallback. The framework does not require a PliegoRS application server.", "Despliega el contenido de target/site en cualquier origen que preserve rutas, MIME types, cache inmutable de assets, el documento 404 con autoría y fallback de rutas limpias. El framework no requiere un servidor de aplicación PliegoRS."),
            note(locale, "Verify after upload", "Compare uploaded bytes against the ledger or release manifest. A successful local build is not evidence that an origin serves the same artifact.", "Verifica después del upload", "Compara los bytes subidos contra el ledger o manifest de release. Un build local exitoso no prueba que un origen sirva el mismo artefacto."),
        ]),
    ].into_view()
}

fn crate_reference(locale: Locale) -> View {
    vec![
        doc_section(locale, "choose", "Choose the owning crate", "Elige el crate propietario", vec![
            definition_list(locale, &[
                ("pliego-dom / pliego-macros", "Escaped views, authored DOM, typed components, SSR adoption, and mounted ownership", "Vistas escapadas, DOM con autoría, componentes tipados, adopción SSR y propiedad montada"),
                ("pliego-log / pliego-fold", "Typed history, schemas, upcasting, projections, replay, effects, and snapshots", "Historia tipada, schemas, upcasting, proyecciones, replay, efectos y snapshots"),
                ("pliego-reactive / pliego-resume", "Owned signals, memos, effects, and resumable standard browser actions", "Signals, memos y effects controlados y acciones estándar reanudables del navegador"),
                ("pliego-content / pliego-assets", "Bounded content ingestion and reproducible adaptive media plans", "Ingesta limitada de contenido y planes reproducibles de media adaptativa"),
                ("pliego-artifact / pliego-ssg / pliego-inspect", "Portable output, documents, routes, receipts, graphs, staged publication, and verification", "Salida portable, documentos, rutas, recibos, grafos, publicación por staging y verificación"),
                ("pliego-adapters / pliego-hyphae", "External browser lifecycle and verified durable sync boundaries", "Lifecycle externo del navegador y límites de sync durable verificado"),
                ("pliego-starters / pliego-cli", "Maintained first-use projects and the complete command surface", "Proyectos mantenidos de primer uso y superficie completa de comandos"),
                ("pliego-router / pliego-runtime (unreleased)", "G1 sealed server routes, bounded HTTP lifecycle, middleware, failures, receipts, and three SSR modes", "Rutas de servidor selladas G1, lifecycle HTTP limitado, middleware, fallos, recibos y tres modos SSR"),
                ("pliego-sdk (unreleased)", "OpenSDK preview manifests, capability admission, Component Model runtime, effect receipts, compatibility, and tooling protocols", "Manifests preview de OpenSDK, admisión de capabilities, runtime Component Model, recibos de efectos, compatibilidad y protocolos de tooling"),
            ]),
            note(locale, "Registry boundary", "The fifteen PliegoRS 0.0.2 packages are published on crates.io. pliego-router, pliego-runtime, and pliego-sdk 0.1.0-preview.1 exist only on main and must be evaluated from a source checkout.", "Límite del registry", "Los quince paquetes PliegoRS 0.0.2 están publicados en crates.io. pliego-router, pliego-runtime y pliego-sdk 0.1.0-preview.1 existen únicamente en main y deben evaluarse desde un checkout de fuentes."),
        ]),
        doc_section(locale, "symbols", "Core public entry points", "Entradas públicas principales", vec![
            definition_list(locale, &[
                ("pliego_ssg::{Site, Page, Head, Asset}", "Author complete documents and publish a deterministic static site", "Crea documentos completos y publica un sitio estático determinista"),
                ("pliego_dom::{View, Element, IntoView, el}", "Compose escaped semantic views; use MountScope for owned browser lifetimes", "Compón vistas semánticas escapadas; usa MountScope para lifetimes controlados del navegador"),
                ("pliego_reactive::{Signal, Memo, Effect}", "Model owned reactive state with equality and disposal semantics", "Modela estado reactivo controlado con semántica de igualdad y disposición"),
                ("pliego_log::{EventSchema, Log, EventCatalogBuilder, SealedEventCatalog}", "Encode typed durable history and seal version admission", "Codifica historia durable tipada y sella admisión de versiones"),
                ("pliego_fold::{Reducer, Projection, ProjectionSnapshot}", "Project accepted events transactionally and restore bound state", "Proyecta eventos aceptados transaccionalmente y restaura estado vinculado"),
                ("pliego_artifact::BuildContext", "Capture exact source identity for verified publication", "Captura identidad exacta de fuentes para publicación verificada"),
                ("pliego_adapters::{AdapterIsland, AdapterPolicy}", "Declare external browser modules and their admission policy", "Declara módulos externos del navegador y su política de admisión"),
                ("pliego_hyphae::{ReceiptVerifier, VerifiedAppendResponse, VerifiedPullPage}", "Cross the durable authority boundary through verified typestate", "Cruza el límite de autoridad durable mediante typestate verificado"),
                ("pliego_runtime::{NativeRuntimeBuilder, CompleteDocument, BoundaryDocument}", "Evaluate the unreleased G1 native lifecycle and three SSR modes from main", "Evalúa desde main el lifecycle nativo G1 y sus tres modos SSR aún no liberados"),
                ("pliego_sdk::{ExtensionManifest, HostContract, CapabilityPolicy}", "Evaluate the unreleased OpenSDK admission boundary from main", "Evalúa desde main el límite de admisión OpenSDK aún no liberado"),
            ]),
        ]),
        doc_section(locale, "rustdoc", "Generate exact-version Rustdoc", "Genera Rustdoc de versión exacta", vec![
            code_block(locale, "shell", "git checkout <accepted-revision>\ncargo doc --workspace --no-deps --locked\n# open target/doc/pliego_ssg/index.html"),
            paragraph(locale, "The repository Rustdoc is the symbol-level reference for an exact revision. This guide explains product contracts and crate ownership; it does not replace signatures, trait bounds, feature flags, or per-item safety notes emitted from the source.", "El Rustdoc del repositorio es la referencia a nivel de símbolos para una revisión exacta. Esta guía explica contratos de producto y propiedad de crates; no reemplaza firmas, trait bounds, feature flags ni notas de seguridad por item emitidas desde la fuente."),
        ]),
        doc_section(locale, "stability", "Respect the pre-1.0 boundary", "Respeta el límite pre-1.0", vec![
            paragraph(locale, "PliegoRS 0.0.2 is public pre-1.0 software. Crate names identify stable ownership boundaries, but public signatures may change between minor releases. Pin one exact released version across every pliego-* dependency and never mix framework versions inside one application graph.", "PliegoRS 0.0.2 es software público pre-1.0. Los nombres de crates identifican límites estables de propiedad, pero las firmas públicas pueden cambiar entre releases menores. Fija una versión liberada exacta en todas las dependencias pliego-* y nunca mezcles versiones del framework dentro del grafo de una aplicación."),
            note(locale, "Published support contract", "The compatibility matrix and changelog define supported toolchains, targets, features, deprecations, and upgrade paths for each release. Linux x64 and ARM64 are the production targets for 0.0.2.", "Contrato de soporte publicado", "La matriz de compatibilidad y el changelog definen toolchains, targets, features, deprecaciones y rutas de upgrade para cada release. Linux x64 y ARM64 son los targets de producción para 0.0.2."),
            note(locale, "G1 and OpenSDK use a separate preview line", "The router, runtime, and OpenSDK 0.1.0-preview.1 may change in another preview protocol. Their implementation does not extend the stability promise of the fifteen published 0.0.2 crates.", "G1 y OpenSDK usan una línea preview separada", "El router, runtime y OpenSDK 0.1.0-preview.1 pueden cambiar en otro protocolo preview. Su implementación no amplía la promesa de estabilidad de los quince crates 0.0.2 publicados."),
        ]),
        doc_section(locale, "boundaries", "Read the normative boundaries", "Lee los límites normativos", vec![
            link_list(locale, &[
                ("https://github.com/celiumsai/pliegors/blob/main/docs/15-framework-api-boundaries.md", "Framework API boundaries", "Límites de API del framework"),
                ("https://github.com/celiumsai/pliegors/blob/main/FRAMEWORK.md", "Architecture and package map", "Arquitectura y mapa de paquetes"),
                ("https://github.com/celiumsai/pliegors/blob/main/CHANGELOG.md", "Source changelog", "Changelog fuente"),
            ]),
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
            "pliego doctor",
            "Diagnose global or project prerequisites with actionable checks",
            "Diagnostica prerrequisitos globales o del proyecto con verificaciones accionables",
        ),
        (
            "pliego report --bundle",
            "Create a redacted local support bundle without uploading it",
            "Crea un bundle local redactado de soporte sin subirlo",
        ),
        (
            "pliego upgrade --check",
            "Evaluate an explicit upgrade plan without modifying the project",
            "Evalúa un plan explícito de upgrade sin modificar el proyecto",
        ),
        (
            "pliego telemetry <status|enable|preview|export|disable>",
            "Control the disabled-by-default local telemetry journal",
            "Controla el journal local de telemetría desactivado por defecto",
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
            "pliego why artifact <path|route>",
            "Verify and explain the causal source-to-artifact chain",
            "Verifica y explica la cadena causal source-to-artifact",
        ),
        (
            "pliego why-rebuilt",
            "Explain the latest bounded development rebuild",
            "Explica el último rebuild limitado de desarrollo",
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
        "developer-loop" => vec![
            ("watch", "Native watcher", "Watcher nativo"),
            ("hmr", "Typed HMR", "HMR tipado"),
            ("explain", "Explain causality", "Explicar causalidad"),
            ("recover", "Failure recovery", "Recuperación de fallos"),
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
        "schemas-and-snapshots" => vec![
            ("catalog", "Event catalog", "Catálogo de eventos"),
            ("upcasting", "Adjacent upcasting", "Upcasting adyacente"),
            ("identity", "Snapshot identity", "Identidad del snapshot"),
            ("restore", "Restore", "Restaurar"),
        ],
        "hyphae-sync" => vec![
            ("boundary", "Optional durability", "Durabilidad opcional"),
            ("append", "Verified append", "Append verificado"),
            ("pull", "Fixed snapshot", "Snapshot fijo"),
            ("authority", "Verified state", "Estado verificado"),
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
        "dom-lifecycle" => vec![
            ("scopes", "Mounted scopes", "Scopes montados"),
            ("keyed", "Keyed identity", "Identidad keyed"),
            ("adoption", "SSR adoption", "Adopción SSR"),
            ("cleanup", "Deterministic cleanup", "Cleanup determinista"),
        ],
        "native-runtime" => vec![
            ("status", "G1 preview status", "Estado del preview G1"),
            ("ownership", "Ownership boundary", "Límite de propiedad"),
            ("rendering", "Three SSR modes", "Tres modos SSR"),
            ("failure", "Limits and failure", "Límites y fallos"),
        ],
        "opensdk" => vec![
            ("status", "Preview status", "Estado del preview"),
            ("admission", "Admission", "Admisión"),
            ("effects", "Effect broker", "Broker de efectos"),
            ("governance", "Governance", "Gobernanza"),
        ],
        "opensdk-components" => vec![
            ("component", "Wasm Component", "Wasm Component"),
            (
                "languages",
                "Language implementations",
                "Implementaciones por lenguaje",
            ),
            ("budgets", "Resource failures", "Fallos de recursos"),
            ("limits", "Evidence boundary", "Límite de evidencia"),
        ],
        "browser-framework-conformance" => vec![
            ("native", "Native runtimes", "Runtimes nativos"),
            ("descriptor", "Admitted descriptor", "Descriptor admitido"),
            ("gate", "Browser gate", "Gate de navegador"),
            (
                "cleanup-evidence",
                "Cleanup evidence",
                "Evidencia de cleanup",
            ),
        ],
        "opensdk-tooling" => vec![
            ("protocol", "Tooling plane", "Plano de tooling"),
            ("editor", "Editor handshake", "Handshake de editor"),
            ("mcp", "MCP client", "Cliente MCP"),
            ("limits", "Protocol limits", "Límites del protocolo"),
        ],
        "opensdk-compatibility" => vec![
            ("matrix", "Compatibility matrix", "Matriz de compatibilidad"),
            ("stability", "Stability tiers", "Niveles de estabilidad"),
            ("deprecation", "Deprecation", "Deprecación"),
            ("source", "Source boundary", "Límite de fuente"),
        ],
        "assets" => vec![
            ("manifest", "Asset manifest", "Manifest de assets"),
            ("profiles", "Profiles", "Perfiles"),
            ("budgets", "Device budgets", "Presupuestos"),
            ("fallbacks", "Fallbacks", "Fallbacks"),
        ],
        "artifact-trust" => vec![
            ("namespace", "Portable namespace", "Namespace portable"),
            ("capture", "Exact inputs", "Entradas exactas"),
            ("receipt", "Receipt and graph", "Recibo y grafo"),
            ("publish", "Staged publication", "Publicación por staging"),
        ],
        "release-trust" => vec![
            ("bytes", "Exact release bytes", "Bytes exactos del release"),
            ("signatures", "Signatures", "Firmas"),
            ("attestations", "SBOM and provenance", "SBOM y provenance"),
            ("promotion", "Promotion", "Promoción"),
        ],
        "performance-evidence" => vec![
            ("protocol", "Measurement protocol", "Protocolo de medición"),
            ("reproduce", "Reproduce", "Reproducir"),
            ("adversarial", "Failure evidence", "Evidencia de fallos"),
            ("interpret", "Interpretation", "Interpretación"),
        ],
        "errors-and-diagnostics" => vec![
            ("browser", "Browser failures", "Fallos en navegador"),
            ("codes", "Diagnostic codes", "Códigos diagnósticos"),
            ("http", "HTTP failures", "Fallos HTTP"),
            ("recovery", "Recovery", "Recuperación"),
        ],
        "telemetry" => vec![
            ("default", "Disabled by default", "Desactivada por defecto"),
            ("allowlist", "Exact allowlist", "Allowlist exacta"),
            ("control", "Preview and export", "Preview y export"),
            ("delete", "Disable and delete", "Desactivar y eliminar"),
        ],
        "build-and-deploy" => vec![
            ("build", "Build", "Build"),
            ("ledger", "Artifact ledger", "Ledger del artefacto"),
            ("releases", "Releases", "Releases"),
            ("deploy", "Deploy", "Desplegar"),
        ],
        "crate-reference" => vec![
            ("choose", "Choose a crate", "Elegir un crate"),
            ("symbols", "Public entry points", "Entradas públicas"),
            ("rustdoc", "Generate Rustdoc", "Generar Rustdoc"),
            ("stability", "Stability", "Estabilidad"),
            ("boundaries", "Normative boundaries", "Límites normativos"),
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
            assert!(outline(topic.slug).len() >= 4);
        }
        assert_eq!(TOPICS.len(), 27);
    }

    #[test]
    fn code_ids_are_stable_and_content_addressed() {
        assert_eq!(stable_hash("pliego dev"), stable_hash("pliego dev"));
        assert_ne!(stable_hash("pliego dev"), stable_hash("pliego build"));
    }
}
