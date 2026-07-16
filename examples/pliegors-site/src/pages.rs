use crate::components::{brand_mark, locale_path};
use crate::content::Locale;
use pliego_dom::{IntoView, View, el};

pub fn home(locale: Locale) -> View {
    View::Fragment(vec![
        hero(locale),
        premise(locale),
        live_contract(locale),
        pipeline(locale),
        invariants(locale),
        adapter_gate(locale),
        material_evidence(locale),
        distribution(locale),
        closing(locale),
    ])
}

fn hero(locale: Locale) -> View {
    let slides = [
        (
            "/media/pliegors/fold-hero.webp",
            "FOLD / 01",
            l(locale, "The execution fold", "El pliegue de ejecución"),
        ),
        (
            "/media/pliegors/ledger-wide.webp",
            "LEDGER / 02",
            l(locale, "The evidence plane", "El plano de evidencia"),
        ),
    ];
    let mut stage = el("div").class("rs-hero__slides");
    for (index, (image, label, kind)) in slides.iter().enumerate() {
        let mut slide = el("figure")
            .class(if index == 0 {
                "rs-hero__slide is-active"
            } else {
                "rs-hero__slide"
            })
            .attr("data-hero-slide", "")
            .attr("data-hero-label", *label)
            .attr("aria-hidden", if index == 0 { "false" } else { "true" });
        let media = if index == 0 {
            el("picture")
                .child(
                    el("source")
                        .attr("media", "(max-width: 680px)")
                        .attr("type", "image/avif")
                        .attr("srcset", "/media/pliegors/fold-portrait.avif"),
                )
                .child(
                    el("source")
                        .attr("media", "(max-width: 680px)")
                        .attr("type", "image/webp")
                        .attr("srcset", "/media/pliegors/fold-portrait.webp"),
                )
                .child(
                    el("source")
                        .attr("type", "image/avif")
                        .attr("srcset", "/media/pliegors/fold-hero.avif"),
                )
                .child(
                    el("img")
                        .attr("src", *image)
                        .attr("fetchpriority", "high")
                        .attr("alt", "")
                        .attr("width", "1672")
                        .attr("height", "941"),
                )
        } else {
            el("picture").child(
                el("img")
                    .attr("data-src", *image)
                    .attr("alt", "")
                    .attr("width", "1536")
                    .attr("height", "1024"),
            )
        };
        slide = slide.child(media).child(
            el("figcaption")
                .child(el("span").child(*kind))
                .child(el("strong").child(*label)),
        );
        stage = stage.child(slide);
    }

    el("section")
        .class("rs-hero")
        .attr("data-hero-carousel", "")
        .attr("data-hero-interval", "7600")
        .child(stage)
        .child(el("div").class("rs-hero__scrim"))
        .child(
            el("div")
                .class("rs-hero__fold-line")
                .attr("aria-hidden", "true"),
        )
        .child(
            el("div")
                .class("rs-hero__register")
                .child(el("span").child("RUST-NATIVE WEB FRAMEWORK"))
                .child(el("span").child(l(
                    locale,
                    "PRIVATE PRE-RELEASE / MEDELLÍN / 2026",
                    "PRE-RELEASE PRIVADO / MEDELLÍN / 2026",
                ))),
        )
        .child(
            el("div")
                .class("rs-hero__content")
                .child(
                    el("div")
                        .class("rs-hero__mark")
                        .attr("aria-hidden", "true")
                        .child(brand_mark(56, None)),
                )
                .child(
                    el("p")
                        .class("utility-label")
                        .child("PLIEGORS / 0.0.1 / CANDIDATE"),
                )
                .child(el("h1").child("Pliego").child(el("em").child("RS")))
                .child(el("p").class("rs-hero__lead").child(l(
                    locale,
                    "The framework owns the system. The browser owns the medium.",
                    "El framework controla el sistema. El navegador conserva el medio.",
                )))
                .child(
                    el("div")
                        .class("rs-actions")
                        .child(action(
                            locale_path(locale, "/docs"),
                            l(locale, "Read the docs", "Leer la documentación"),
                            true,
                        ))
                        .child(action(
                            "https://github.com/celiumsai/pliegors",
                            l(locale, "Inspect the source", "Inspeccionar el código"),
                            false,
                        )),
                ),
        )
        .child(
            el("div")
                .class("rs-hero__controls pliego-cover-carousel-ui")
                .child(icon_button(
                    "data-hero-previous",
                    l(locale, "Previous frame", "Cuadro anterior"),
                    "←",
                ))
                .child(
                    el("span")
                        .child(el("b").attr("data-hero-current", "").child("01"))
                        .child(" / 02"),
                )
                .child(
                    el("span")
                        .attr("data-hero-current-label", "")
                        .child("FOLD / 01"),
                )
                .child(icon_button(
                    "data-hero-next",
                    l(locale, "Next frame", "Cuadro siguiente"),
                    "→",
                ))
                .child(
                    el("button")
                        .attr("type", "button")
                        .attr("data-hero-pause", "")
                        .attr(
                            "data-hero-pause-label",
                            l(locale, "Pause carousel", "Pausar carrusel"),
                        )
                        .attr(
                            "data-hero-resume-label",
                            l(locale, "Resume carousel", "Reanudar carrusel"),
                        )
                        .attr("aria-label", l(locale, "Pause carousel", "Pausar carrusel"))
                        .attr("aria-pressed", "false")
                        .child(el("span").attr("data-hero-pause-icon", "").child("Ⅱ"))
                        .child(el("span").attr("data-hero-play-icon", "").child("▶")),
                ),
        )
        .child(
            el("dl")
                .class("rs-hero__rail")
                .child(hero_fact(
                    "01",
                    "STATIC FIRST",
                    l(locale, "Useful HTML", "HTML útil"),
                ))
                .child(hero_fact(
                    "02",
                    "RUST + WASM",
                    l(locale, "Resume by intent", "Reanuda por intención"),
                ))
                .child(hero_fact("03", "ADAPTERS", "GSAP / LENIS / THREE.JS"))
                .child(hero_fact(
                    "04",
                    "EVIDENCE",
                    l(locale, "Deterministic output", "Salida determinista"),
                )),
        )
        .into_view()
}

fn hero_fact(index: &str, term: &str, description: &str) -> View {
    el("div")
        .child(el("span").child(index.to_owned()))
        .child(el("dt").child(term.to_owned()))
        .child(el("dd").child(description.to_owned()))
        .into_view()
}

fn premise(locale: Locale) -> View {
    el("section")
        .class("rs-premise")
        .child(section_code("01", l(locale, "The position", "La posición")))
        .child(el("h2").child(l(
            locale,
            "A website should carry a point of view without carrying a runtime it does not need.",
            "Un sitio debe sostener un punto de vista sin cargar un runtime que no necesita.",
        )))
        .child(
            el("div")
                .class("rs-premise__copy")
                .child(el("p").child(l(
                    locale,
                    "PliegoRS authors routes, content contracts, reactive state, asset policy, and build evidence in Rust. Useful HTML arrives first. Browser work resumes only where the document asks for it.",
                    "PliegoRS define rutas, contratos de contenido, estado reactivo, política de assets y evidencia del build en Rust. El HTML útil llega primero. El navegador reanuda sólo donde el documento lo solicita.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Established browser libraries stay native JavaScript behind explicit lifecycle adapters. PliegoRS protects the contract while leaving the visual language entirely open.",
                    "Las librerías consolidadas del navegador siguen siendo JavaScript nativo detrás de adaptadores de lifecycle explícitos. PliegoRS protege el contrato y deja completamente abierto el lenguaje visual.",
                ))),
        )
        .into_view()
}

fn live_contract(locale: Locale) -> View {
    el("section")
        .class("rs-contract")
        .child(
            el("header")
                .class("rs-section-head")
                .child(section_code("02", l(locale, "Live contract", "Contrato vivo")))
                .child(el("h2").child(l(
                    locale,
                    "One authored source. Three inspectable states.",
                    "Una fuente con autoría. Tres estados inspeccionables.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Move through the same document as source, deterministic artifact, and admitted browser behavior.",
                    "Recorre el mismo documento como fuente, artefacto determinista y comportamiento admitido en el navegador.",
                ))),
        )
        .child(engine_lab(locale))
        .into_view()
}

fn engine_lab(locale: Locale) -> View {
    el("div")
        .class("engine-lab")
        .attr("data-engine-lab", "")
        .child(
            el("div")
                .class("engine-lab__head")
                .child(el("span").child("PLIEGO / LIVE CONTRACT"))
                .child(
                    el("div")
                        .class("engine-stage-tabs")
                        .attr("role", "tablist")
                        .attr(
                            "aria-label",
                            l(locale, "Compilation states", "Estados de compilación"),
                        )
                        .child(stage_button("source", "01 SOURCE", true))
                        .child(stage_button("build", "02 BUILD", false))
                        .child(stage_button("runtime", "03 RUNTIME", false)),
                ),
        )
        .child(
            el("div")
                .class("engine-lab__body")
                .child(
                    el("div")
                        .class("engine-lab__mark")
                        .attr("aria-hidden", "true")
                        .child(brand_mark(170, None))
                        .child(el("span").child("SOURCE → OUTPUT")),
                )
                .child(
                    el("div")
                        .class("engine-panels")
                        .child(engine_panel(
                            "source",
                            true,
                            "src/pages/home.rs",
                            &[
                                ("01", "fn home() -> View {"),
                                ("02", "  el(\"main\")"),
                                ("03", "    .child(hero())"),
                                ("04", "    .child(contract())"),
                                ("05", "    .into_view()"),
                                ("06", "}"),
                            ],
                        ))
                        .child(engine_panel(
                            "build",
                            false,
                            "target/site/pliego-ledger.json",
                            &[
                                ("OK", "routes ........ deterministic"),
                                ("OK", "content ....... typed + bounded"),
                                ("OK", "assets ........ adaptive plan"),
                                ("OK", "seo ........... canonical + JSON-LD"),
                                ("OUT", "HTML / CSS / JS / WASM"),
                                ("SHA", "content-addressed build ledger"),
                            ],
                        ))
                        .child(engine_panel(
                            "runtime",
                            false,
                            "browser / admission policy",
                            &[
                                ("01", "useful HTML arrives first"),
                                ("02", "Rust/WASM resumes by intent"),
                                ("03", "capabilities admit adapters"),
                                ("04", "libraries mount natively"),
                                ("05", "reduced motion preserves meaning"),
                                ("06", "cleanup runs automatically"),
                            ],
                        )),
                ),
        )
        .child(
            el("div")
                .class("engine-lab__foot")
                .child(el("span").child("NO HIDDEN APP RUNTIME"))
                .child(el("span").child("HOST-OWNED OUTPUT")),
        )
        .into_view()
}

fn stage_button(stage: &str, label: &str, active: bool) -> View {
    let mut button = el("button")
        .id(format!("engine-stage-{stage}"))
        .attr("type", "button")
        .attr("role", "tab")
        .attr("data-engine-stage", stage)
        .attr("aria-controls", format!("engine-panel-{stage}"))
        .attr("aria-selected", if active { "true" } else { "false" })
        .attr("tabindex", if active { "0" } else { "-1" })
        .child(label.to_owned());
    if active {
        button = button.class("is-active");
    }
    button.into_view()
}

fn engine_panel(stage: &str, active: bool, filename: &str, lines: &[(&str, &str)]) -> View {
    let mut list = el("ol");
    for (number, code) in lines {
        list = list.child(
            el("li")
                .child(el("span").child(*number))
                .child(el("code").child(*code)),
        );
    }
    let mut panel = el("div")
        .class(if active {
            "engine-panel is-active"
        } else {
            "engine-panel"
        })
        .id(format!("engine-panel-{stage}"))
        .attr("role", "tabpanel")
        .attr("data-engine-panel", stage)
        .attr("aria-labelledby", format!("engine-stage-{stage}"))
        .child(
            el("div")
                .class("engine-panel__file")
                .child(el("i"))
                .child(filename.to_owned()),
        )
        .child(list);
    if !active {
        panel = panel.attr("hidden", "");
    }
    panel.into_view()
}

fn pipeline(locale: Locale) -> View {
    let steps = [
        (
            "AUTHOR",
            l(locale, "Own the document", "Controla el documento"),
            l(
                locale,
                "Routes, views, content schemas, events, and asset intent begin as explicit Rust source.",
                "Rutas, vistas, schemas de contenido, eventos e intención de assets comienzan como fuente Rust explícita.",
            ),
            "src → graph",
        ),
        (
            "COMPILE",
            l(
                locale,
                "Make output accountable",
                "Haz responsable la salida",
            ),
            l(
                locale,
                "The build resolves content, routes, media policy, metadata, and a content-addressed ledger.",
                "El build resuelve contenido, rutas, política de medios, metadata y un ledger dirigido por contenido.",
            ),
            "graph → artifact",
        ),
        (
            "RESUME",
            l(
                locale,
                "Wake only what moves",
                "Despierta sólo lo que se mueve",
            ),
            l(
                locale,
                "Useful HTML is already present. Rust/WASM resumes owned state at intentional boundaries.",
                "El HTML útil ya está presente. Rust/WASM reanuda estado controlado en límites intencionales.",
            ),
            "artifact → interface",
        ),
        (
            "EXTEND",
            l(locale, "Admit the medium", "Admite el medio"),
            l(
                locale,
                "GSAP, Lenis, Three.js, WebGL, and future tools enter through a versioned lifecycle contract.",
                "GSAP, Lenis, Three.js, WebGL y herramientas futuras entran por un contrato de lifecycle versionado.",
            ),
            "interface → expression",
        ),
    ];
    let mut list = el("ol").class("rs-pipeline__steps");
    for (index, (label, title, body, output)) in steps.into_iter().enumerate() {
        list = list.child(
            el("li")
                .class(if index == 0 { "is-active" } else { "" })
                .attr("data-pipeline-step", "")
                .attr("data-pipeline-index", index.to_string())
                .child(el("span").child(format!("0{}", index + 1)))
                .child(
                    el("div")
                        .child(el("p").class("utility-label").child(label))
                        .child(el("h3").child(title))
                        .child(el("p").child(body))
                        .child(el("code").child(output)),
                ),
        );
    }
    el("section")
        .class("rs-pipeline")
        .attr("data-pipeline", "")
        .child(
            el("header")
                .class("rs-section-head")
                .child(section_code("03", l(locale, "The route", "El recorrido")))
                .child(el("h2").child(l(
                    locale,
                    "From authored source to expressive surface.",
                    "De fuente con autoría a superficie expresiva.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Every transition is visible. Every boundary has an owner.",
                    "Cada transición es visible. Cada límite tiene un responsable.",
                ))),
        )
        .child(
            el("div")
                .class("rs-pipeline__body")
                .child(
                    el("aside")
                        .class("rs-pipeline__instrument")
                        .attr("aria-hidden", "true")
                        .child(
                            el("div")
                                .class("rs-pipeline__glyph")
                                .child(brand_mark(180, None)),
                        )
                        .child(
                            el("div")
                                .class("rs-pipeline__progress")
                                .child(el("span").attr("data-pipeline-progress", "")),
                        )
                        .child(el("p").child("AUTHOR / COMPILE / RESUME / EXTEND")),
                )
                .child(list),
        )
        .into_view()
}

fn invariants(locale: Locale) -> View {
    let items = [
        (
            "01",
            "DETERMINISTIC OUTPUT",
            l(
                locale,
                "The same declared inputs produce the same route graph and evidence ledger.",
                "Las mismas entradas declaradas producen el mismo grafo de rutas y ledger de evidencia.",
            ),
        ),
        (
            "02",
            "USEFUL HTML FIRST",
            l(
                locale,
                "Meaning, navigation, and essential controls do not wait for a client runtime.",
                "El significado, la navegación y los controles esenciales no esperan un runtime cliente.",
            ),
        ),
        (
            "03",
            "EXPLICIT LIFECYCLE",
            l(
                locale,
                "Mount, update, unmount, abort, and cleanup are part of the adapter contract.",
                "Mount, update, unmount, abort y cleanup forman parte del contrato de adapters.",
            ),
        ),
        (
            "04",
            "EVIDENCE UNDERNEATH",
            l(
                locale,
                "Routes, assets, source revision, and output hashes remain inspectable after build.",
                "Rutas, assets, revisión fuente y hashes de salida permanecen inspeccionables después del build.",
            ),
        ),
    ];
    let mut grid = el("div").class("rs-invariants__grid");
    for (number, title, body) in items {
        grid = grid.child(
            el("article")
                .attr("data-reveal", "")
                .child(el("span").child(number))
                .child(el("h3").child(title))
                .child(el("p").child(body)),
        );
    }
    el("section")
        .class("rs-invariants")
        .child(
            el("header")
                .child(section_code(
                    "04",
                    l(locale, "The invariants", "Las invariantes"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Expression on the surface. Discipline underneath.",
                    "Expresión en la superficie. Disciplina debajo.",
                ))),
        )
        .child(grid)
        .into_view()
}

fn adapter_gate(locale: Locale) -> View {
    let libraries = ["GSAP", "LENIS", "THREE.JS", "WEBGL"];
    let policies = [
        l(locale, "Lazy load", "Carga diferida"),
        l(locale, "Motion policy", "Política de movimiento"),
        l(locale, "Capability tier", "Nivel de capacidad"),
        l(locale, "Automatic cleanup", "Limpieza automática"),
    ];
    let mut sources = el("ul").class("rs-gate__libraries");
    for library in libraries {
        sources = sources.child(el("li").child(library));
    }
    let mut outputs = el("ul").class("rs-gate__policies");
    for policy in policies {
        outputs = outputs.child(el("li").child(policy));
    }
    el("section")
        .class("rs-gate")
        .child(
            el("header")
                .child(section_code("05", l(locale, "Browser medium", "Medio del navegador")))
                .child(el("h2").child(l(
                    locale,
                    "Use the best tools in their natural state.",
                    "Usa las mejores herramientas en su estado natural.",
                )))
                .child(el("p").child(l(
                    locale,
                    "PliegoRS does not reinvent mature browser libraries. It gives them a narrow, observable, and disposable boundary.",
                    "PliegoRS no reinventa librerías maduras del navegador. Les da un límite estrecho, observable y descartable.",
                ))),
        )
        .child(
            el("div")
                .class("rs-gate__diagram")
                .child(sources)
                .child(
                    el("div")
                        .class("rs-gate__contract")
                        .attr("aria-label", l(locale, "Adapter contract", "Contrato de adapters"))
                        .child(brand_mark(72, None))
                        .child(el("span").child("ADAPTER / V1")),
                )
                .child(outputs),
        )
        .into_view()
}

fn material_evidence(locale: Locale) -> View {
    el("section")
        .class("rs-material")
        .child(
            el("picture")
                .child(
                    el("source")
                        .attr("type", "image/avif")
                        .attr("srcset", "/media/pliegors/ledger-wide.avif"),
                )
                .child(
                    el("img")
                        .attr("src", "/media/pliegors/ledger-wide.webp")
                        .attr("alt", l(locale, "Folded glass, metal, and mineral planes crossed by a registration line", "Planos plegados de vidrio, metal y mineral atravesados por una línea de registro"))
                        .attr("loading", "lazy")
                        .attr("width", "1536")
                        .attr("height", "1024"),
                ),
        )
        .child(
            el("div")
                .class("rs-material__copy")
                .child(section_code("06", l(locale, "Build evidence", "Evidencia del build")))
                .child(el("h2").child(l(
                    locale,
                    "The artifact should remember how it came to exist.",
                    "El artefacto debe recordar cómo llegó a existir.",
                )))
                .child(el("p").child(l(
                    locale,
                    "A PliegoRS build records the route graph, source revision, asset variants, hashes, and toolchain identity. The visible page is only one layer of the result.",
                    "Un build de PliegoRS registra el grafo de rutas, la revisión fuente, variantes de assets, hashes e identidad del toolchain. La página visible es sólo una capa del resultado.",
                ))),
        )
        .into_view()
}

fn distribution(locale: Locale) -> View {
    el("section")
        .class("rs-distribution")
        .child(
            el("div")
                .child(section_code("07", l(locale, "Distribution", "Distribución")))
                .child(el("h2").child(l(locale, "Built once. Verified per target.", "Compilado una vez. Verificado por target.")))
                .child(el("p").child(l(
                    locale,
                    "GitHub Releases is the canonical channel. Linux artifacts are production targets; macOS and Windows builds support local development. The accepted private candidate has complete R0-R7 evidence; public availability begins only when an explicit release is published.",
                    "GitHub Releases es el canal canónico. Los artefactos Linux son targets de producción; los builds de macOS y Windows sirven al desarrollo local. El candidato privado aceptado tiene evidencia completa de R0-R7; la disponibilidad pública comienza únicamente cuando se publique un release explícito.",
                ))),
        )
        .child(
            el("pre")
                .class("rs-terminal")
                .attr("aria-label", l(locale, "PliegoRS installation example", "Ejemplo de instalación de PliegoRS"))
                .child(el("code").child("$ pliego new field-notes\n$ cd field-notes\n$ pliego dev --lan\n\nPLIEGORS  local  http://127.0.0.1:4300\n          network http://0.0.0.0:4300")),
        )
        .into_view()
}

fn closing(locale: Locale) -> View {
    el("section")
        .class("rs-closing")
        .child(brand_mark(64, None))
        .child(el("p").class("utility-label").child("PLIEGORS / MEDELLÍN"))
        .child(el("h2").child(l(locale, "The authored web, with a memory.", "La web con autoría, y con memoria.")))
        .child(el("p").child(l(
            locale,
            "The private candidate has completed R0-R7 hardening: documented, inspectable, independently exercised, and owned from source to output.",
            "El candidato privado completó el fortalecimiento R0-R7: documentado, inspeccionable, ejercitado de forma independiente y controlado de fuente a salida.",
        )))
        .child(action("mailto:hello@pliegors.dev", "hello@pliegors.dev", true))
        .into_view()
}

pub fn about(locale: Locale) -> View {
    View::Fragment(vec![
        page_hero(
            "ABOUT / PLIEGORS",
            l(
                locale,
                "The web is a medium. Reliability is a system.",
                "La web es un medio. La confiabilidad es un sistema.",
            ),
            l(
                locale,
                "PliegoRS is a Rust-native framework for authored interfaces whose state, output, and behavior can be inspected and reproduced.",
                "PliegoRS es un framework Rust-native para interfaces con autoría cuyo estado, salida y comportamiento pueden inspeccionarse y reproducirse.",
            ),
        ),
        about_origin(locale),
        about_position(locale),
        about_stewardship(locale),
        about_release(locale),
        closing(locale),
    ])
}

fn about_origin(locale: Locale) -> View {
    el("section")
        .class("rs-about-origin")
        .child(section_code(
            "01",
            l(locale, "Why it exists", "Por qué existe"),
        ))
        .child(
            el("div")
                .class("rs-about-origin__statement")
                .child(el("h2").child(l(
                    locale,
                    "Expressive sites should not become opaque systems.",
                    "Los sitios expresivos no deberían convertirse en sistemas opacos.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Modern browser work can be visually ambitious and operationally fragile at the same time. PliegoRS begins from a different premise: state is a fold of typed events, delivery is a verifiable artifact, and enhancement is attached through explicit lifecycles.",
                    "El trabajo moderno en el navegador puede ser visualmente ambicioso y operacionalmente frágil al mismo tiempo. PliegoRS parte de otra premisa: el estado es un fold de eventos tipados, la entrega es un artefacto verificable y la mejora se conecta mediante lifecycles explícitos.",
                ))),
        )
        .child(
            el("figure")
                .child(
                    el("picture")
                        .child(
                            el("source")
                                .attr("type", "image/avif")
                                .attr("srcset", "/media/pliegors/ledger-wide.avif"),
                        )
                        .child(
                            el("img")
                                .attr("src", "/media/pliegors/ledger-wide.webp")
                                .attr("alt", "")
                                .attr("loading", "lazy")
                                .attr("width", "1536")
                                .attr("height", "1024"),
                        ),
                )
                .child(el("figcaption").child(l(
                    locale,
                    "The ledger is part of the interface, not an implementation detail.",
                    "El ledger es parte de la interfaz, no un detalle de implementación.",
                ))),
        )
        .into_view()
}

fn about_position(locale: Locale) -> View {
    let positions = [
        (
            "EVENTS",
            l(
                locale,
                "History before mutation",
                "Historia antes que mutación",
            ),
            l(
                locale,
                "Typed events preserve intent. Folds derive state. Replay verifies the result.",
                "Los eventos tipados preservan intención. Los folds derivan estado. El replay verifica el resultado.",
            ),
        ),
        (
            "OUTPUT",
            l(
                locale,
                "Artifacts before claims",
                "Artefactos antes que promesas",
            ),
            l(
                locale,
                "HTML, assets, manifests, checksums, and diagnostics make delivery inspectable.",
                "HTML, assets, manifests, checksums y diagnósticos hacen inspeccionable la entrega.",
            ),
        ),
        (
            "BROWSER",
            l(
                locale,
                "Lifecycle before glue",
                "Lifecycle antes que pegamento",
            ),
            l(
                locale,
                "Rust/WASM owns application behavior. Mature JavaScript libraries remain behind capability and cleanup contracts.",
                "Rust/WASM controla el comportamiento. Las librerías JavaScript maduras permanecen detrás de contratos de capacidad y limpieza.",
            ),
        ),
        (
            "DATA",
            l(
                locale,
                "Boundaries before lock-in",
                "Límites antes que encierro",
            ),
            l(
                locale,
                "Static projects stay static. Durable projects can use Hyphae through a versioned, verified protocol.",
                "Los proyectos estáticos permanecen estáticos. Los proyectos durables pueden usar Hyphae mediante un protocolo versionado y verificable.",
            ),
        ),
    ];
    let mut register = el("div").class("rs-about-position__register");
    for (index, (code, title, body)) in positions.into_iter().enumerate() {
        register = register.child(
            el("article")
                .child(el("span").child(format!("{:02} / {code}", index + 1)))
                .child(el("h3").child(title))
                .child(el("p").child(body)),
        );
    }

    el("section")
        .class("rs-about-position")
        .child(
            el("div")
                .class("rs-about-position__head")
                .child(section_code(
                    "02",
                    l(locale, "The position", "La posición"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Four boundaries. One coherent framework.",
                    "Cuatro límites. Un framework coherente.",
                )))
                .child(el("p").child(l(
                    locale,
                    "PliegoRS does not replace the browser, the graphics ecosystem, or a durable data plane. It makes their responsibilities explicit.",
                    "PliegoRS no reemplaza el navegador, el ecosistema gráfico ni un plano de datos durable. Hace explícitas sus responsabilidades.",
                ))),
        )
        .child(register)
        .into_view()
}

fn about_stewardship(locale: Locale) -> View {
    el("section")
        .class("rs-about-stewardship")
        .child(
            el("div")
                .class("rs-about-stewardship__mark")
                .attr("aria-hidden", "true")
                .child(brand_mark(112, None)),
        )
        .child(
            el("div")
                .child(section_code(
                    "03",
                    l(locale, "Stewardship", "Stewardship"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Open source with accountable authorship.",
                    "Código abierto con autoría responsable.",
                )))
                .child(el("p").child(l(
                    locale,
                    "PliegoRS is stewarded in Medellín by Celiums Solutions LLC. The source is Apache-2.0; the project name and symbol remain protected marks. Governance, security reporting, compatibility, and releases are documented public contracts.",
                    "PliegoRS es administrado desde Medellín por Celiums Solutions LLC. El código usa Apache-2.0; el nombre y el símbolo del proyecto siguen siendo marcas protegidas. Gobernanza, reportes de seguridad, compatibilidad y releases son contratos públicos documentados.",
                )))
                .child(
                    el("div")
                        .class("rs-actions")
                        .child(action(
                            locale_path(locale, "/security"),
                            l(locale, "Security policy", "Política de seguridad"),
                            true,
                        ))
                        .child(action(
                            "https://github.com/celiumsai/pliegors/blob/main/GOVERNANCE.md",
                            l(locale, "Governance", "Gobernanza"),
                            false,
                        )),
                ),
        )
        .into_view()
}

fn about_release(locale: Locale) -> View {
    el("section")
        .class("rs-about-release")
        .child(section_code(
            "04",
            l(locale, "Current state", "Estado actual"),
        ))
        .child(el("p").class("utility-label").child("0.0.1 / CANDIDATE"))
        .child(el("h2").child(l(
            locale,
            "The first release is evidence, not a countdown.",
            "El primer release es evidencia, no una cuenta regresiva.",
        )))
        .child(el("p").child(l(
            locale,
            "Source, documentation, security posture, platform builds, checksums, and R0-R7 evidence now agree on the accepted private candidate. Publication remains a separate product decision; no public install channel or support promise exists until a release is explicitly published.",
            "El código, la documentación, la postura de seguridad, los builds por plataforma, los checksums y la evidencia R0-R7 ya coinciden en el candidato privado aceptado. La publicación sigue siendo una decisión de producto separada; no existe canal público de instalación ni promesa de soporte hasta publicar un release de forma explícita.",
        )))
        .child(
            el("div")
                .class("rs-actions")
                .child(action(
                    locale_path(locale, "/docs/getting-started"),
                    l(locale, "Start with the contract", "Comenzar con el contrato"),
                    true,
                ))
                .child(action(
                    locale_path(locale, "/changelog"),
                    l(locale, "Read the changelog", "Leer el changelog"),
                    false,
                )),
        )
        .into_view()
}

pub fn changelog(locale: Locale) -> View {
    let entries = [
        (
            "0.0.1",
            "2026-07-16",
            l(
                locale,
                "Accepted private candidate",
                "Candidato privado aceptado",
            ),
            l(
                locale,
                "Native SSG, typed content, Rust/WASM runtime, adapters, adaptive assets, verified Hyphae boundary, signed five-target distribution, and an independently exercised external application.",
                "SSG nativo, contenido tipado, runtime Rust/WASM, adaptadores, assets adaptativos, límite Hyphae verificado, distribución firmada para cinco targets y una aplicación externa ejercitada de forma independiente.",
            ),
        ),
        (
            "R5–R7",
            "ACCEPTED",
            l(
                locale,
                "Delivery and external proof",
                "Entrega y prueba externa",
            ),
            l(
                locale,
                "Causal HMR, why commands, replayable first use, signed reproducible candidates, installer lifecycle, and an external durable application passed their acceptance gates.",
                "HMR causal, comandos why, primer uso reproducible, candidatos firmados y reproducibles, lifecycle de instaladores y una aplicación durable externa aprobaron sus gates de aceptación.",
            ),
        ),
        (
            "R0–R4",
            "ACCEPTED",
            l(locale, "Core hardening", "Fortalecimiento del núcleo"),
            l(
                locale,
                "Reactive safety, artifact trust, verified sync, schema and snapshot identity, keyed reconciliation, SSR adoption, and deterministic DOM cleanup passed their acceptance gates.",
                "Seguridad reactiva, confianza de artefactos, sync verificado, identidad de schemas y snapshots, reconciliación keyed, adopción SSR y cleanup determinista del DOM aprobaron sus gates de aceptación.",
            ),
        ),
    ];
    let mut list = el("div").class("rs-change-list");
    for (version, date, stage, summary) in entries {
        list = list.child(
            el("article")
                .child(el("span").child(date))
                .child(el("h2").child(version))
                .child(el("strong").child(stage))
                .child(el("p").child(summary)),
        );
    }
    View::Fragment(vec![
        page_hero(
            "CHANGELOG",
            l(
                locale,
                "Changes with evidence attached.",
                "Cambios con evidencia adjunta.",
            ),
            l(
                locale,
                "Only completed, testable framework work belongs here.",
                "Aquí sólo entra trabajo del framework completo y verificable.",
            ),
        ),
        list.into_view(),
    ])
}

pub fn security(locale: Locale) -> View {
    let items = [
        (
            l(locale, "Report privately", "Reporta en privado"),
            l(
                locale,
                "Send reproducible details to hello@pliegors.dev. Do not publish an unpatched exploit.",
                "Envía detalles reproducibles a hello@pliegors.dev. No publiques un exploit sin corregir.",
            ),
        ),
        (
            l(locale, "Trust boundaries", "Límites de confianza"),
            l(
                locale,
                "Route output, content inputs, plugin capabilities, replay chains, and release artifacts are validated at explicit boundaries.",
                "La salida de rutas, entradas de contenido, capacidades de plugins, cadenas de replay y artefactos de release se validan en límites explícitos.",
            ),
        ),
        (
            l(locale, "Release integrity", "Integridad de releases"),
            l(
                locale,
                "Candidates require target checksums, a final manifest, reproducibility evidence, and a private review before publication.",
                "Los candidatos requieren checksums por target, manifest final, evidencia de reproducibilidad y revisión privada antes de publicarse.",
            ),
        ),
    ];
    framework_page(
        locale,
        "SECURITY",
        l(
            locale,
            "Fail loudly at trust boundaries.",
            "Fallar con claridad en los límites de confianza.",
        ),
        l(
            locale,
            "Security is part of the framework contract, not a launch-week checklist.",
            "La seguridad es parte del contrato del framework, no una lista para la semana del lanzamiento.",
        ),
        items,
    )
}

pub fn accessibility(locale: Locale) -> View {
    let items = [
        (
            l(locale, "Useful HTML first", "HTML útil primero"),
            l(
                locale,
                "Navigation and meaning remain available before client code starts.",
                "La navegación y el significado permanecen disponibles antes de iniciar código cliente.",
            ),
        ),
        (
            l(locale, "Motion is optional", "El movimiento es opcional"),
            l(
                locale,
                "Reduced-motion and capability policy preserve content, order, and controls.",
                "Las políticas de movimiento reducido y capacidades conservan contenido, orden y controles.",
            ),
        ),
        (
            l(locale, "Keyboard and focus", "Teclado y foco"),
            l(
                locale,
                "Interactive surfaces expose stable focus order, labels, and visible focus states.",
                "Las superficies interactivas exponen orden de foco estable, etiquetas y estados de foco visibles.",
            ),
        ),
    ];
    framework_page(
        locale,
        "ACCESSIBILITY",
        l(
            locale,
            "The baseline is usable before it is spectacular.",
            "La base es usable antes de ser espectacular.",
        ),
        l(
            locale,
            "PliegoRS treats accessibility as an output invariant and tests progressive enhancement against it.",
            "PliegoRS trata la accesibilidad como una invariante de salida y prueba la mejora progresiva contra ella.",
        ),
        items,
    )
}

pub fn legal_hub(locale: Locale) -> View {
    let links = [
        (
            "terms",
            l(locale, "Terms", "Términos"),
            l(
                locale,
                "Rules for using the website and pre-release software.",
                "Reglas de uso del sitio y del software previo al release.",
            ),
        ),
        (
            "privacy",
            l(locale, "Privacy", "Privacidad"),
            l(
                locale,
                "Current data handling and contact disclosure.",
                "Tratamiento actual de datos y divulgación de contacto.",
            ),
        ),
        (
            "cookies",
            "Cookies",
            l(
                locale,
                "Local preferences and the absence of advertising trackers.",
                "Preferencias locales y ausencia de trackers publicitarios.",
            ),
        ),
        (
            "acceptable-use",
            l(locale, "Acceptable use", "Uso aceptable"),
            l(
                locale,
                "Boundaries for source, binaries, infrastructure, and reports.",
                "Límites para código, binarios, infraestructura y reportes.",
            ),
        ),
    ];
    let mut grid = el("div").class("rs-legal-grid");
    for (slug, title, body) in links {
        grid = grid.child(
            el("a")
                .attr("href", locale_path(locale, &format!("/legal/{slug}")))
                .child(el("h2").child(title))
                .child(el("p").child(body))
                .child(el("span").child("↗")),
        );
    }
    View::Fragment(vec![
        page_hero(
            "LEGAL / CURRENT",
            l(
                locale,
                "Plain language. Narrow scope.",
                "Lenguaje claro. Alcance limitado.",
            ),
            l(
                locale,
                "These documents cover the PliegoRS website and private pre-release program.",
                "Estos documentos cubren el sitio de PliegoRS y el programa privado previo al release.",
            ),
        ),
        grid.into_view(),
    ])
}

pub fn legal_document(locale: Locale, slug: &str) -> Result<View, String> {
    let (title_en, title_es, intro_en, intro_es, sections) = match slug {
        "terms" => (
            "Terms",
            "Términos",
            "These terms govern access to the PliegoRS website and private pre-release materials.",
            "Estos términos rigen el acceso al sitio de PliegoRS y a materiales privados previos al release.",
            vec![
                (
                    "01",
                    "Current status",
                    "Estado actual",
                    "PliegoRS is pre-release software. Documentation and interfaces may change until a public version is explicitly published.",
                    "PliegoRS es software previo al release. La documentación y las interfaces pueden cambiar hasta que se publique explícitamente una versión pública.",
                ),
                (
                    "02",
                    "License",
                    "Licencia",
                    "Repository access does not grant rights beyond the license files and written permissions attached to the material you receive.",
                    "El acceso al repositorio no concede derechos más allá de las licencias y permisos escritos adjuntos al material recibido.",
                ),
                (
                    "03",
                    "No warranty",
                    "Sin garantía",
                    "Pre-release software is provided for evaluation without warranties to the extent permitted by law.",
                    "El software previo al release se entrega para evaluación sin garantías en la medida permitida por la ley.",
                ),
            ],
        ),
        "privacy" => (
            "Privacy",
            "Privacidad",
            "The current static website collects no account, payment, or advertising profile.",
            "El sitio estático actual no recopila cuentas, pagos ni perfiles publicitarios.",
            vec![
                (
                    "01",
                    "Local data",
                    "Datos locales",
                    "The browser may store appearance preferences locally. That value is not sent to PliegoRS.",
                    "El navegador puede guardar localmente la preferencia de apariencia. Ese valor no se envía a PliegoRS.",
                ),
                (
                    "02",
                    "Email",
                    "Correo",
                    "Messages sent to hello@pliegors.dev are processed to answer the request and maintain necessary correspondence.",
                    "Los mensajes enviados a hello@pliegors.dev se procesan para responder la solicitud y conservar la correspondencia necesaria.",
                ),
                (
                    "03",
                    "Infrastructure",
                    "Infraestructura",
                    "Hosting and repository providers may process standard security and access logs under their own terms.",
                    "Los proveedores de hosting y repositorio pueden procesar logs estándar de seguridad y acceso bajo sus propios términos.",
                ),
            ],
        ),
        "cookies" => (
            "Cookies",
            "Cookies",
            "The current website uses no advertising or cross-site tracking cookies.",
            "El sitio actual no usa cookies publicitarias ni de rastreo entre sitios.",
            vec![
                (
                    "01",
                    "Appearance",
                    "Apariencia",
                    "A local storage value remembers system, light, or dark appearance. You can reset it from your browser.",
                    "Un valor de almacenamiento local recuerda apariencia de sistema, clara u oscura. Puedes restablecerlo desde tu navegador.",
                ),
                (
                    "02",
                    "Future changes",
                    "Cambios futuros",
                    "Any material change in analytics or storage must be disclosed here before activation.",
                    "Cualquier cambio material en analítica o almacenamiento debe informarse aquí antes de activarse.",
                ),
            ],
        ),
        "acceptable-use" => (
            "Acceptable use",
            "Uso aceptable",
            "Use PliegoRS materials without harming people, systems, or the integrity of the project.",
            "Usa los materiales de PliegoRS sin perjudicar a personas, sistemas o la integridad del proyecto.",
            vec![
                (
                    "01",
                    "Security",
                    "Seguridad",
                    "Do not probe private infrastructure, bypass access controls, publish unpatched exploits, or distribute modified binaries as official artifacts.",
                    "No pruebes infraestructura privada, eludas controles de acceso, publiques exploits sin corregir ni distribuyas binarios modificados como artefactos oficiales.",
                ),
                (
                    "02",
                    "Identity",
                    "Identidad",
                    "Do not impersonate PliegoRS, Celiums Solutions LLC, maintainers, or official release channels.",
                    "No suplantes a PliegoRS, Celiums Solutions LLC, mantenedores ni canales oficiales de release.",
                ),
                (
                    "03",
                    "Reports",
                    "Reportes",
                    "Good-faith reports with reproducible evidence are welcome at hello@pliegors.dev.",
                    "Los reportes de buena fe con evidencia reproducible son bienvenidos en hello@pliegors.dev.",
                ),
            ],
        ),
        _ => return Err(format!("unsupported legal document {slug}")),
    };
    let mut body = el("div").class("rs-legal-document__body");
    for (number, en_title, es_title, en_body, es_body) in sections {
        body = body.child(
            el("section")
                .child(el("span").child(number))
                .child(el("h2").child(l(locale, en_title, es_title)))
                .child(el("p").child(l(locale, en_body, es_body))),
        );
    }
    Ok(View::Fragment(vec![
        page_hero(
            "LEGAL / 2026-07-14",
            l(locale, title_en, title_es),
            l(locale, intro_en, intro_es),
        ),
        body.into_view(),
    ]))
}

pub fn not_found(locale: Locale) -> View {
    el("section")
        .class("rs-not-found")
        .child(brand_mark(52, None))
        .child(el("p").class("utility-label").child("HTTP / 404"))
        .child(el("h1").child(l(
            locale,
            "This route is not in the graph.",
            "Esta ruta no está en el grafo.",
        )))
        .child(action(
            locale_path(locale, "/"),
            l(locale, "Return home", "Volver al inicio"),
            true,
        ))
        .into_view()
}

fn framework_page<const N: usize>(
    locale: Locale,
    label: &str,
    title: &str,
    lead: &str,
    items: [(&str, &str); N],
) -> View {
    let mut grid = el("div").class("rs-principle-grid");
    for (index, (heading, body)) in items.into_iter().enumerate() {
        grid = grid.child(
            el("article")
                .child(el("span").child(format!("{:02}", index + 1)))
                .child(el("h2").child(heading))
                .child(el("p").child(body)),
        );
    }
    View::Fragment(vec![
        page_hero(label, title, lead),
        grid.into_view(),
        closing(locale),
    ])
}

fn page_hero(label: &str, title: &str, lead: &str) -> View {
    el("section")
        .class("rs-page-hero")
        .child(el("p").class("utility-label").child(label.to_owned()))
        .child(el("h1").child(title.to_owned()))
        .child(el("p").child(lead.to_owned()))
        .into_view()
}

fn section_code(number: &str, label: &str) -> View {
    el("div")
        .class("rs-section-code")
        .child(el("span").child(format!("PLG.RS / {number}")))
        .child(el("span").child(label.to_owned()))
        .into_view()
}

fn action(href: impl Into<String>, label: &str, solid: bool) -> View {
    el("a")
        .class(if solid {
            "rs-button rs-button--solid"
        } else {
            "rs-button"
        })
        .attr("href", href.into())
        .child(label.to_owned())
        .child(el("span").attr("aria-hidden", "true").child("↗"))
        .into_view()
}

fn icon_button(attribute: &str, label: &str, glyph: &str) -> View {
    el("button")
        .attr("type", "button")
        .attr(attribute, "")
        .attr("aria-label", label)
        .attr("title", label)
        .child(glyph.to_owned())
        .into_view()
}

fn l<'a>(locale: Locale, en: &'a str, es: &'a str) -> &'a str {
    if locale.is_spanish() { es } else { en }
}
