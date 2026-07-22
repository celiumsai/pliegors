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
                    "PUBLIC BETA / MEDELLÍN / 2026",
                    "BETA PÚBLICA / MEDELLÍN / 2026",
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
                        .child("PLIEGORS / 0.2.0-BETA.1 / PUBLIC BETA"),
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
                    "PliegoRS 0.2.0-beta.1 is public on crates.io and GitHub Releases. Linux artifacts are production targets; macOS and Windows builds support local development. The release carries complete R0-R7, P8, G1, and G2 evidence.",
                    "PliegoRS 0.2.0-beta.1 está disponible en crates.io y GitHub Releases. Los artefactos Linux son targets de producción; los builds de macOS y Windows sirven al desarrollo local. El release contiene la evidencia R0-R7, P8, G1 y G2 completa.",
                ))),
        )
        .child(
            el("pre")
                .class("rs-terminal")
                .attr("aria-label", l(locale, "PliegoRS installation example", "Ejemplo de instalación de PliegoRS"))
                .child(el("code").child("$ cargo install pliego-cli --version 0.2.0-beta.1 --locked\n$ pliego new field-notes\n$ cd field-notes\n$ pliego dev\n\nPLIEGORS  local  http://127.0.0.1:4400")),
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
            "The public beta carries completed R0-R7 and P8 hardening plus G1 and G2: documented, inspectable, independently exercised, and owned from source to output.",
            "La beta pública incorpora R0-R7 y P8 completos más G1 y G2: documentada, inspeccionable, ejercitada de forma independiente y controlada de fuente a salida.",
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
        .child(el("p").class("utility-label").child("0.2.0-BETA.1 / PUBLIC BETA"))
        .child(el("h2").child(l(
            locale,
            "The current release is a reproducible claim.",
            "El release actual es una afirmación reproducible.",
        )))
        .child(el("p").child(l(
            locale,
            "Source, documentation, 19 crates.io packages, five platform builds, signed attestations, the nine-environment golden matrix, and R0-R7, P8, G1, plus G2 evidence agree on the 0.2.0-beta.1 public beta.",
            "El código, la documentación, 19 paquetes de crates.io, cinco builds de plataforma, attestations firmadas, la matriz golden de nueve entornos y la evidencia R0-R7, P8, G1 más G2 coinciden en la beta pública 0.2.0-beta.1.",
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
    View::Fragment(vec![
        page_hero(
            "CHANGELOG",
            l(
                locale,
                "A public record, not a victory lap.",
                "Un registro público, no una vuelta de victoria.",
            ),
            l(
                locale,
                "Released evidence stays separate from experimental work. Every entry names what changed, what passed, and what remains unsettled.",
                "La evidencia liberada permanece separada del trabajo experimental. Cada entrada nombra qué cambió, qué aprobó y qué sigue sin resolverse.",
            ),
        ),
        changelog_overview(locale),
        el("section")
            .class("rs-change-list")
            .attr(
                "aria-label",
                l(locale, "Release history", "Historial de releases"),
            )
            .child(change_entry(
                locale,
                "v0-2-0-beta-1",
                "0.2.0-beta.1",
                "2026-07-22",
                Some("2026-07-22"),
                l(locale, "Coordinated public beta", "Beta pública coordinada"),
                l(locale, "The package graph becomes one product.", "El grafo de paquetes se convierte en un producto."),
                l(
                    locale,
                    "All nineteen framework crates share one exact version. The CLI, G1 native runtime, G2 data contracts, and OpenSDK preview now move together while G3 remains explicitly unreleased.",
                    "Los diecinueve crates del framework comparten una versión exacta. El CLI, runtime nativo G1, contratos de datos G2 y preview OpenSDK avanzan juntos mientras G3 sigue explícitamente no liberado.",
                ),
                &[
                    l(locale, "G2 publishes loaders, actions, sessions, idempotency, cache policy, invalidation, and redacted diagnostics.", "G2 publica loaders, actions, sesiones, idempotencia, política de caché, invalidación y diagnósticos redactados."),
                    l(locale, "The signed five-target release preserves R0-R7 and P8 gates and adds G1/G2 conformance evidence.", "El release firmado para cinco targets preserva los gates R0-R7 y P8 y agrega evidencia de conformidad G1/G2."),
                    l(locale, "PBOC and the Cloudflare application runtime remain G3 work rather than implied beta capabilities.", "PBOC y el runtime de aplicaciones Cloudflare siguen siendo trabajo G3, no capacidades beta implícitas."),
                ],
                Some((
                    "https://github.com/celiumsai/pliegors/releases/tag/v0.2.0-beta.1",
                    l(locale, "Verify release 0.2.0-beta.1", "Verificar el release 0.2.0-beta.1"),
                )),
            ))
            .child(change_entry(
                locale,
                "preview-components-v0-1-0-preview-1",
                "0.1.0-preview.1",
                "2026-07-21",
                Some("2026-07-21"),
                l(locale, "Component prerelease", "Prerelease de componentes"),
                l(locale, "The native runtime becomes installable.", "El runtime nativo se vuelve instalable."),
                l(
                    locale,
                    "pliego-router, pliego-runtime, and pliego-sdk are public at 0.1.0-preview.1. G1 is complete, while the complete CLI remains 0.0.2 and that tagged release contains neither G2 nor G3.",
                    "pliego-router, pliego-runtime y pliego-sdk son públicos en 0.1.0-preview.1. G1 está completo, mientras el CLI completo sigue en 0.0.2 y ese release etiquetado no contiene G2 ni G3.",
                ),
                &[
                    l(locale, "Bounded HTTP/1.1 and HTTP/2 transport with connection admission, slow-peer deadlines, graceful drain, and overload behavior.", "Transporte HTTP/1.1 y HTTP/2 limitado con admisión de conexiones, deadlines para peers lentos, drain ordenado y comportamiento de sobrecarga."),
                    l(locale, "Complete, ordered, and async-boundary SSR with route-owned complete and streamed layouts under one output budget.", "SSR complete, ordered y async-boundary con layouts complete y streamed controlados por rutas bajo un único presupuesto de salida."),
                    l(locale, "Structured completion events and operator-enabled OTel exclude request values and isolate operator callback panics.", "Eventos estructurados de finalización y OTel habilitado por el operador excluyen valores del request y aíslan panics de callbacks del operador."),
                    l(locale, "The ASVS 5.0 ownership map, real-socket adversarial corpus, fixed-load RSS harness, CodeQL, fuzzing, Chromium, and package reconstruction passed.", "El mapa de ownership ASVS 5.0, corpus adversarial de sockets reales, harness RSS de carga fija, CodeQL, fuzzing, Chromium y reconstrucción de paquetes aprobaron."),
                    l(locale, "OpenSDK build/browser/tooling is public preview; its server plane and governance decisions remain pending.", "OpenSDK build/browser/tooling es preview público; su plano de servidor y decisiones de gobernanza siguen pendientes."),
                ],
                Some((
                    "https://github.com/celiumsai/pliegors/releases/tag/preview-components-v0.1.0-preview.1",
                    l(locale, "Open the component release", "Abrir el release de componentes"),
                )),
            ))
            .child(change_entry(
                locale,
                "v0-0-2",
                "0.0.2",
                "2026-07-18",
                Some("2026-07-18"),
                l(locale, "Public preview", "Preview público"),
                l(locale, "Trust becomes part of the toolchain.", "La confianza pasa a ser parte del toolchain."),
                l(
                    locale,
                    "P8 closes the gap between a working framework and one that can be independently evaluated, installed, diagnosed, and verified.",
                    "P8 cierra la distancia entre un framework funcional y uno que puede evaluarse, instalarse, diagnosticarse y verificarse de forma independiente.",
                ),
                &[
                    l(locale, "15 crates at 0.0.2, five platform targets, 28 signed release assets, SBOM, provenance, and Sigstore identity.", "15 crates en 0.0.2, cinco targets de plataforma, 28 assets firmados, SBOM, provenance e identidad Sigstore."),
                    l(locale, "Nine-environment golden matrix including WSL2, Unicode paths, long paths, and a pinned container.", "Matriz golden de nueve entornos, incluyendo WSL2, rutas Unicode, rutas largas y un contenedor fijado."),
                    l(locale, "Doctor, deterministic support reports, upgrade checks, bounded fuzzing, reproducible benchmarks, and opt-in local telemetry.", "Doctor, reportes de soporte deterministas, checks de upgrade, fuzzing acotado, benchmarks reproducibles y telemetría local opt-in."),
                ],
                Some((
                    "https://github.com/celiumsai/pliegors/releases/tag/v0.0.2",
                    l(locale, "Verify release 0.0.2", "Verificar el release 0.0.2"),
                )),
            ))
            .child(change_entry(
                locale,
                "v0-0-1",
                "0.0.1",
                "2026-07-16",
                Some("2026-07-16"),
                l(locale, "First public preview", "Primer preview público"),
                l(locale, "The framework leaves the workshop.", "El framework sale del taller."),
                l(
                    locale,
                    "The first public release established the Rust-native framework, authored developer experience, and accepted R0-R7 evidence baseline.",
                    "El primer release público estableció el framework nativo en Rust, la experiencia de desarrollo con autoría y la línea base de evidencia R0-R7 aceptada.",
                ),
                &[
                    l(locale, "Native SSG, typed views and content, event folds, reactive runtime, DOM lifecycle, adapters, assets, and Hyphae boundary.", "SSG nativo, views y contenido tipados, event folds, runtime reactivo, lifecycle DOM, adaptadores, assets y límite Hyphae."),
                    l(locale, "Signed five-target distribution, authored error pages, official starters, bilingual documentation, and external flagship evidence.", "Distribución firmada para cinco targets, páginas de error con autoría, starters oficiales, documentación bilingüe y evidencia flagship externa."),
                ],
                Some((
                    "https://github.com/celiumsai/pliegors/releases/tag/v0.0.1",
                    l(locale, "Verify release 0.0.1", "Verificar el release 0.0.1"),
                )),
            ))
            .into_view(),
    ])
}

fn changelog_overview(locale: Locale) -> View {
    let facts = [
        (
            l(locale, "Current release", "Release actual"),
            "0.2.0-beta.1",
        ),
        (l(locale, "Published", "Publicado"), "2026-07-22"),
        (l(locale, "Published crates", "Crates publicados"), "19"),
        (l(locale, "Release channel", "Canal de release"), "BETA"),
    ];
    let mut fact_list = el("dl").class("rs-changelog-overview__facts");
    for (label, value) in facts {
        fact_list = fact_list.child(
            el("div")
                .child(el("dt").child(label))
                .child(el("dd").child(value)),
        );
    }
    el("section")
        .class("rs-changelog-overview")
        .child(
            el("div")
                .class("rs-changelog-overview__copy")
                .child(el("p").class("utility-label").child("RELEASE LEDGER / 003"))
                .child(el("h2").child(l(
                    locale,
                    "The latest claim is the one you can verify.",
                    "La afirmación más reciente es la que puedes verificar.",
                )))
                .child(el("p").child(l(
                    locale,
                    "The website summarizes the record. GitHub keeps the canonical Markdown, immutable tags, signatures, attestations, and downloadable bytes.",
                    "El sitio resume el registro. GitHub conserva el Markdown canónico, los tags inmutables, firmas, attestations y bytes descargables.",
                )))
                .child(
                    el("div")
                        .class("rs-actions")
                        .child(action(
                            "https://github.com/celiumsai/pliegors/releases/tag/v0.2.0-beta.1",
                            l(locale, "Open latest release", "Abrir el último release"),
                            true,
                        ))
                        .child(action(
                            "https://github.com/celiumsai/pliegors/blob/main/CHANGELOG.md",
                            l(locale, "Read source changelog", "Leer changelog fuente"),
                            false,
                        )),
                ),
        )
        .child(fact_list)
        .into_view()
}

#[allow(clippy::too_many_arguments)]
fn change_entry(
    locale: Locale,
    id: &str,
    version: &str,
    date_label: &str,
    datetime: Option<&str>,
    status: &str,
    title: &str,
    summary: &str,
    bullets: &[&str],
    source: Option<(&str, &str)>,
) -> View {
    let date = if let Some(datetime) = datetime {
        el("time")
            .attr("datetime", datetime)
            .child(date_label.to_owned())
    } else {
        el("span").child(date_label.to_owned())
    };
    let mut details = el("ul").class("rs-change-entry__details");
    for bullet in bullets {
        details = details.child(el("li").child((*bullet).to_owned()));
    }
    let mut body = el("div")
        .class("rs-change-entry__body")
        .child(el("h3").child(title.to_owned()))
        .child(el("p").child(summary.to_owned()))
        .child(details);
    if let Some((href, label)) = source {
        body = body.child(
            el("a")
                .class("rs-change-entry__link")
                .attr("href", href)
                .child(label.to_owned())
                .child(el("span").attr("aria-hidden", "true").child("↗")),
        );
    }
    el("article")
        .id(id)
        .attr("data-change-entry", id)
        .child(
            el("div")
                .class("rs-change-entry__meta")
                .child(date)
                .child(el("span").child(status.to_owned())),
        )
        .child(
            el("div")
                .class("rs-change-entry__version")
                .child(el("span").child(l(locale, "Version", "Versión")))
                .child(el("h2").child(version.to_owned())),
        )
        .child(body)
        .into_view()
}

pub fn security(locale: Locale) -> View {
    View::Fragment(vec![
        security_hero(locale),
        security_posture(locale),
        security_boundaries(locale),
        security_evidence(locale),
        security_supply_chain(locale),
        security_limitations(locale),
        security_support(locale),
        security_report(locale),
    ])
}

fn security_hero(locale: Locale) -> View {
    el("section")
        .class("rs-security-hero")
        .child(
            el("picture")
                .class("rs-security-hero__media")
                .child(
                    el("source")
                        .attr("type", "image/avif")
                        .attr("srcset", "/media/pliegors/security-trust.avif"),
                )
                .child(
                    el("img")
                        .attr("src", "/media/pliegors/security-trust.webp")
                        .attr(
                            "alt",
                            l(
                                locale,
                                "Five interlocking planes of glass, metal, stone, resin, and chrome surrounding a protected center",
                                "Cinco planos de vidrio, metal, piedra, resina y cromo alrededor de un centro protegido",
                            ),
                        )
                        .attr("width", "1600")
                        .attr("height", "900")
                        .attr("fetchpriority", "high"),
                ),
        )
        .child(
            el("div")
                .class("rs-security-hero__scrim")
                .attr("aria-hidden", "true"),
        )
        .child(
            el("div")
                .class("rs-security-hero__register")
                .child(el("span").child("PLIEGORS / TRUST CENTER"))
                .child(el("span").child(l(
                    locale,
                    "REVIEWED / 2026-07-16",
                    "REVISADO / 2026-07-16",
                ))),
        )
        .child(
            el("div")
                .class("rs-security-hero__content")
                .child(el("p").class("utility-label").child("SECURITY / R0-R7"))
                .child(el("h1").child(l(
                    locale,
                    "Trust is verified. Not implied.",
                    "La confianza se verifica. No se presume.",
                )))
                .child(el("p").class("rs-security-hero__lead").child(l(
                    locale,
                    "PliegoRS treats content, builds, plugins, replay, and distribution as explicit trust boundaries with bounded inputs and inspectable evidence.",
                    "PliegoRS trata contenido, builds, plugins, replay y distribución como límites explícitos de confianza con entradas acotadas y evidencia inspeccionable.",
                )))
                .child(
                    el("div")
                        .class("rs-actions")
                        .child(action(
                            "mailto:hello@pliegors.dev?subject=SECURITY%3A%20PliegoRS%20report",
                            l(locale, "Report privately", "Reportar en privado"),
                            true,
                        ))
                        .child(action(
                            "https://github.com/celiumsai/pliegors/blob/main/SECURITY.md",
                            l(locale, "Read the policy", "Leer la política"),
                            false,
                        )),
                ),
        )
        .child(
            el("div")
                .class("rs-security-hero__status")
                .attr(
                    "aria-label",
                    l(locale, "Current security posture", "Postura actual de seguridad"),
                )
                .child(
                    el("span")
                        .child("19")
                        .child(el("small").child(l(locale, "findings closed", "hallazgos cerrados"))),
                )
                .child(
                    el("span")
                        .child("R0–R7")
                        .child(el("small").child(l(locale, "evidence accepted", "evidencia aceptada"))),
                )
                .child(
                    el("span")
                        .child("PUBLIC BETA")
                        .child(el("small").child(l(locale, "0.2.0-beta.1 supported", "0.2.0-beta.1 soportado"))),
                ),
        )
        .into_view()
}

fn security_posture(locale: Locale) -> View {
    let metrics = [
        (
            "19",
            l(locale, "Findings closed", "Hallazgos cerrados"),
            l(locale, "1 P0 · 12 P1 · 6 P2", "1 P0 · 12 P1 · 6 P2"),
        ),
        (
            "0",
            l(
                locale,
                "Known vulnerable packages",
                "Paquetes vulnerables conocidos",
            ),
            l(
                locale,
                "Current audit · 2026-07-16 · 1 maintenance warning",
                "Auditoría actual · 2026-07-16 · 1 alerta de mantenimiento",
            ),
        ),
        (
            "5",
            l(
                locale,
                "Native targets reproduced",
                "Targets nativos reproducidos",
            ),
            l(
                locale,
                "Two binary replicas per target",
                "Dos réplicas binarias por target",
            ),
        ),
        (
            "15",
            l(locale, "Signed primary assets", "Assets primarios firmados"),
            l(
                locale,
                "One exact Ed25519 manifest",
                "Un manifest Ed25519 de conjunto exacto",
            ),
        ),
    ];
    let mut grid = el("dl").class("rs-security-posture__grid");
    for (value, label, detail) in metrics {
        grid = grid.child(
            el("div")
                .attr("data-reveal", "")
                .child(el("dt").child(label))
                .child(el("dd").child(value))
                .child(el("p").child(detail)),
        );
    }
    el("section")
        .class("rs-security-posture")
        .child(
            el("header")
                .child(section_code(
                    "01",
                    l(locale, "Current posture", "Postura actual"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "A snapshot with a date, not a permanent promise.",
                    "Una captura con fecha, no una promesa permanente.",
                )))
                .child(el("p").child(l(
                    locale,
                    "These numbers describe the 0.2.0-beta.1 public beta and its frozen dependency graph. Every release must reproduce the same gates against its own bytes.",
                    "Estas cifras describen la beta pública 0.2.0-beta.1 y su grafo congelado de dependencias. Cada release debe reproducir los mismos gates contra sus propios bytes.",
                ))),
        )
        .child(grid)
        .child(action(
            "https://github.com/celiumsai/pliegors/blob/main/docs/26-security-plugins-and-adaptive-media.md",
            l(locale, "Inspect the hardening review", "Inspeccionar la revisión de seguridad"),
            false,
        ))
        .into_view()
}

fn security_boundaries(locale: Locale) -> View {
    let boundaries = [
        (
            "01",
            l(locale, "Filesystem + content", "Filesystem + contenido"),
            l(
                locale,
                "Root capabilities, no-follow opens, canonical confinement, and byte, depth, count, and graph ceilings.",
                "Capabilities de raíz, aperturas no-follow, confinamiento canónico y límites de bytes, profundidad, cantidad y grafo.",
            ),
            locale_path(locale, "/docs/content"),
        ),
        (
            "02",
            l(locale, "Build + artifact", "Build + artefacto"),
            l(
                locale,
                "Guarded staging, rollback replacement, exact output receipts, causal graphs, and verification before explanation.",
                "Staging protegido, reemplazo con rollback, recibos exactos de salida, grafos causales y verificación antes de explicar.",
            ),
            locale_path(locale, "/docs/artifact-trust"),
        ),
        (
            "03",
            l(locale, "Plugins + browser", "Plugins + navegador"),
            l(
                locale,
                "Capability admission, same-origin modules, bounded props, serialized updates, reduced motion, and awaited cleanup.",
                "Admisión por capacidades, módulos same-origin, props acotadas, updates serializados, movimiento reducido y cleanup esperado.",
            ),
            locale_path(locale, "/docs/dom-lifecycle"),
        ),
        (
            "04",
            l(locale, "Replay + sync", "Replay + sync"),
            l(
                locale,
                "Canonical signatures, explicit authority, contiguous cursors, fork rejection, and verified pages before replay.",
                "Firmas canónicas, autoridad explícita, cursores contiguos, rechazo de forks y páginas verificadas antes del replay.",
            ),
            locale_path(locale, "/docs/hyphae-sync"),
        ),
        (
            "05",
            l(locale, "Release + install", "Release + instalación"),
            l(
                locale,
                "Exact asset sets, two-replica binary agreement, detached Ed25519 signatures, lifecycle smoke tests, and explicit promotion.",
                "Conjuntos exactos de assets, acuerdo binario de dos réplicas, firmas Ed25519 separadas, smoke tests de lifecycle y promoción explícita.",
            ),
            locale_path(locale, "/docs/build-and-deploy"),
        ),
    ];
    let mut list = el("ol").class("rs-security-boundaries__map");
    for (number, title, body, href) in boundaries {
        list = list.child(
            el("li")
                .attr("data-security-boundary", number)
                .attr("data-reveal", "")
                .child(el("span").child(number))
                .child(el("h3").child(title))
                .child(el("p").child(body))
                .child(
                    el("a")
                        .attr("href", href)
                        .child(l(locale, "Open contract", "Abrir contrato"))
                        .child(el("span").attr("aria-hidden", "true").child("↗")),
                ),
        );
    }
    el("section")
        .class("rs-security-boundaries")
        .child(
            el("header")
                .child(section_code(
                    "02",
                    l(locale, "Trust topology", "Topología de confianza"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Five boundaries. No invisible handoff.",
                    "Cinco límites. Ningún traspaso invisible.",
                )))
                .child(el("p").child(l(
                    locale,
                    "A value becomes trusted only after the boundary that owns its risk has admitted it. Failure stops publication, replay, import, or installation.",
                    "Un valor sólo se vuelve confiable cuando el límite que controla su riesgo lo admite. Un fallo detiene publicación, replay, import o instalación.",
                ))),
        )
        .child(
            el("div")
                .class("rs-security-boundaries__core")
                .attr("aria-hidden", "true")
                .child(brand_mark(62, None))
                .child(el("span").child("FAIL / CLOSED")),
        )
        .child(list)
        .into_view()
}

fn security_evidence(locale: Locale) -> View {
    let rows = [
        (
            "P0",
            l(locale, "Destructive output", "Salida destructiva"),
            l(
                locale,
                "Output paths cannot target the project, an ancestor, a link, or an unowned directory.",
                "Las rutas de salida no pueden apuntar al proyecto, un ancestro, un link ni un directorio sin ownership.",
            ),
            "R1 / ARTIFACT TRUST",
        ),
        (
            "P1",
            l(locale, "Preview confinement", "Confinamiento de preview"),
            l(
                locale,
                "Loopback by default, bounded workers and queue, finite heartbeats, no linked-file traversal.",
                "Loopback por defecto, workers y cola acotados, heartbeats finitos y sin traversal mediante links.",
            ),
            "SECURITY / REVIEW",
        ),
        (
            "P1",
            l(locale, "Adapter races", "Carreras de adapters"),
            l(
                locale,
                "Generation tokens, serialized updates, cancellation, terminal guards, and awaited teardown prevent revival after disposal.",
                "Tokens de generación, updates serializados, cancelación, guards terminales y teardown esperado impiden revivir tras dispose.",
            ),
            "R4 / DOM LIFECYCLE",
        ),
        (
            "P1",
            l(locale, "Verified replay", "Replay verificado"),
            l(
                locale,
                "Unknown authorities, invalid signatures, gaps, forks, and unsupported event versions fail before replay state exists.",
                "Autoridades desconocidas, firmas inválidas, gaps, forks y versiones no soportadas fallan antes de que exista estado de replay.",
            ),
            "R2 / VERIFIED SYNC",
        ),
        (
            "P2",
            l(locale, "Resource exhaustion", "Agotamiento de recursos"),
            l(
                locale,
                "Content, manifests, media recipes, props, graphs, ledgers, sources, and staged output have explicit ceilings.",
                "Contenido, manifests, recetas de media, props, grafos, ledgers, fuentes y salida staged tienen límites explícitos.",
            ),
            "SECURITY / REVIEW",
        ),
        (
            "R6",
            l(locale, "Distribution mutation", "Mutación de distribución"),
            l(
                locale,
                "Changed bytes, extras, missing files, key drift, sidecar drift, and replica disagreement reject the release candidate.",
                "Bytes alterados, extras, archivos faltantes, drift de key, sidecars o réplicas rechazan el candidato de release.",
            ),
            "R6 / DISTRIBUTION",
        ),
    ];
    let mut body = el("tbody");
    for (severity, surface, control, evidence) in rows {
        body = body.child(
            el("tr")
                .attr("data-security-evidence", severity)
                .child(el("td").attr("data-label", "ID").child(severity))
                .child(
                    el("th")
                        .attr("scope", "row")
                        .attr("data-label", l(locale, "Surface", "Superficie"))
                        .child(surface),
                )
                .child(
                    el("td")
                        .attr(
                            "data-label",
                            l(locale, "Enforced control", "Control aplicado"),
                        )
                        .child(control),
                )
                .child(
                    el("td")
                        .attr("data-label", l(locale, "Evidence", "Evidencia"))
                        .child(evidence),
                )
                .child(
                    el("td")
                        .attr("data-label", l(locale, "State", "Estado"))
                        .child(l(locale, "CLOSED", "CERRADO")),
                ),
        );
    }
    el("section")
        .class("rs-security-evidence")
        .child(
            el("header")
                .child(section_code(
                    "03",
                    l(locale, "Evidence ledger", "Ledger de evidencia"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Security evidence, not decorative badges.",
                    "Evidencia de seguridad, no badges decorativos.",
                )))
                .child(el("p").child(l(
                    locale,
                    "The public ledger summarizes representative controls. The repository preserves the complete threat models, adversarial fixtures, commands, and residual risks.",
                    "El ledger público resume controles representativos. El repositorio conserva threat models completos, fixtures adversariales, comandos y riesgos residuales.",
                ))),
        )
        .child(
            el("div")
                .class("rs-security-evidence__table")
                .child(
                    el("table")
                        .child(
                            el("thead").child(
                                el("tr")
                                    .child(el("th").attr("scope", "col").child("ID"))
                                    .child(el("th").attr("scope", "col").child(l(locale, "Surface", "Superficie")))
                                    .child(el("th").attr("scope", "col").child(l(locale, "Enforced control", "Control aplicado")))
                                    .child(el("th").attr("scope", "col").child(l(locale, "Evidence", "Evidencia")))
                                    .child(el("th").attr("scope", "col").child(l(locale, "State", "Estado"))),
                            ),
                        )
                        .child(body),
                ),
        )
        .child(
            el("div")
                .class("rs-security-evidence__links")
                .child(action(
                    "https://github.com/celiumsai/pliegors/blob/main/docs/26-security-plugins-and-adaptive-media.md",
                    l(locale, "Complete security review", "Revisión completa de seguridad"),
                    false,
                ))
                .child(action(
                    "https://github.com/celiumsai/pliegors/tree/main/docs/evidence",
                    l(locale, "R0-R7 evidence", "Evidencia R0-R7"),
                    false,
                )),
        )
        .into_view()
}

fn security_supply_chain(locale: Locale) -> View {
    el("section")
        .class("rs-security-supply")
        .child(
            el("div")
                .class("rs-security-supply__copy")
                .child(section_code(
                    "04",
                    l(locale, "Release trust", "Confianza del release"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "The manifest signs the set, not the story.",
                    "El manifest firma el conjunto, no la historia.",
                )))
                .child(el("p").child(l(
                    locale,
                    "The accepted release binds 15 primary assets, five ordered targets, two binary replicas per target, source commit, sizes, hashes, roles, and a detached Ed25519 signature.",
                    "El release aceptado vincula 15 assets primarios, cinco targets ordenados, dos réplicas binarias por target, commit fuente, tamaños, hashes, roles y una firma Ed25519 separada.",
                )))
                .child(
                    el("dl")
                        .class("rs-security-supply__facts")
                        .child(
                            el("div")
                                .child(el("dt").child(l(locale, "Key ID", "ID de key")))
                                .child(el("dd").child("pliegors-candidate-2026-01")),
                        )
                        .child(
                            el("div")
                                .child(el("dt").child(l(locale, "Algorithm", "Algoritmo")))
                                .child(el("dd").child("Ed25519")),
                        )
                        .child(
                            el("div")
                                .child(el("dt").child(l(locale, "Install lifecycle", "Lifecycle de instalación")))
                                .child(el("dd").child("install → execute → rollback → uninstall")),
                        ),
                )
                .child(el("p").class("rs-security-supply__boundary").child(l(
                    locale,
                    "Boundary: direct installers verify checksum sidecars. Verify the complete signed bundle against this independently published fingerprint for the high-assurance path.",
                    "Límite: los instaladores directos verifican sidecars de checksum. Para la ruta de mayor garantía, verifica el bundle firmado completo contra este fingerprint publicado de forma independiente.",
                ))),
        )
        .child(
            el("div")
                .class("rs-security-supply__verification")
                .child(el("span").class("utility-label").child("RELEASE / TRUST ROOT"))
                .child(el("code").class("rs-security-fingerprint").child(
                    "sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250",
                ))
                .child(
                    el("pre")
                        .attr(
                            "aria-label",
                            l(locale, "Release verification command", "Comando de verificación de release"),
                        )
                        .child(el("code").child("node verify-release-bundle.mjs \\\n  --dir . \\\n  --expected-key-fingerprint \\\n  sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250")),
                )
                .child(action(
                    "https://github.com/celiumsai/pliegors/blob/main/docs/33-candidate-distribution-contract.md",
                    l(locale, "Read the distribution contract", "Leer el contrato de distribución"),
                    false,
                )),
        )
        .into_view()
}

fn security_limitations(locale: Locale) -> View {
    let items = [
        (
            "01",
            l(
                locale,
                "External JavaScript is not sandboxed",
                "JavaScript externo no está sandboxed",
            ),
            l(
                locale,
                "Capability admission and lifecycle cleanup reduce integration risk; they do not make arbitrary third-party code trustworthy.",
                "La admisión por capacidades y el cleanup de lifecycle reducen riesgo de integración; no vuelven confiable código arbitrario de terceros.",
            ),
        ),
        (
            "02",
            l(
                locale,
                "No future server claim",
                "Sin afirmaciones sobre un server futuro",
            ),
            l(
                locale,
                "The completed review covers the static Rust/WASM surface. A server runtime, credentials, and product backend require their own threat models.",
                "La revisión completada cubre la superficie estática Rust/WASM. Un runtime server, credenciales y backend de producto requieren sus propios threat models.",
            ),
        ),
        (
            "03",
            l(
                locale,
                "Integrity is not authority",
                "Integridad no es autoridad",
            ),
            l(
                locale,
                "A content hash detects change. It does not identify a signer, grant permission, or establish provenance without an authority policy.",
                "Un hash de contenido detecta cambios. No identifica a un firmante, concede permisos ni establece provenance sin una política de autoridad.",
            ),
        ),
        (
            "04",
            l(
                locale,
                "Installer signature verification is external",
                "La verificación de firma del instalador es externa",
            ),
            l(
                locale,
                "Network installers verify archive sidecars but do not independently verify the detached Ed25519 signature. The complete bundle verifier is the high-assurance path.",
                "Los instaladores de red verifican sidecars del archivo, pero no validan por sí mismos la firma Ed25519 separada. El verificador del bundle completo es la ruta de mayor garantía.",
            ),
        ),
        (
            "05",
            l(
                locale,
                "One transitive crate is unmaintained",
                "Un crate transitivo no tiene mantenimiento",
            ),
            l(
                locale,
                "RustSec RUSTSEC-2026-0173 flags the build-time proc-macro-error2 dependency through rstml. No vulnerability is reported; removal remains tracked as a maintenance item.",
                "RustSec RUSTSEC-2026-0173 señala la dependencia de build proc-macro-error2 a través de rstml. No reporta una vulnerabilidad; su eliminación sigue rastreada como mantenimiento.",
            ),
        ),
    ];
    let mut grid = el("div").class("rs-security-limitations__grid");
    for (number, title, body) in items {
        grid = grid.child(
            el("article")
                .attr("data-security-limitation", number)
                .attr("data-reveal", "")
                .child(el("span").child(number))
                .child(el("h3").child(title))
                .child(el("p").child(body)),
        );
    }
    el("section")
        .class("rs-security-limitations")
        .child(
            el("header")
                .child(section_code(
                    "05",
                    l(locale, "Claim boundary", "Límite de afirmaciones"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "What PliegoRS does not claim matters too.",
                    "Lo que PliegoRS no afirma también importa.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Security documentation becomes dangerous when it turns a scoped control into a universal promise.",
                    "La documentación de seguridad se vuelve peligrosa cuando convierte un control acotado en una promesa universal.",
                ))),
        )
        .child(grid)
        .into_view()
}

fn security_support(locale: Locale) -> View {
    el("section")
        .class("rs-security-support")
        .child(
            el("div")
                .class("rs-security-support__versions")
                .child(section_code(
                    "06",
                    l(locale, "Support window", "Ventana de soporte"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Supported means named and bounded.",
                    "Soportado significa nombrado y acotado.",
                )))
                .child(
                    el("table")
                        .child(
                            el("thead").child(
                                el("tr")
                                    .child(el("th").attr("scope", "col").child(l(locale, "Line", "Línea")))
                                    .child(el("th").attr("scope", "col").child(l(locale, "Status", "Estado")))
                                    .child(el("th").attr("scope", "col").child(l(locale, "Security fixes", "Correcciones"))),
                            ),
                        )
                        .child(
                            el("tbody").child(
                                el("tr")
                                    .child(el("th").attr("scope", "row").child("0.2.0-beta.1"))
                                    .child(el("td").child(l(locale, "Public beta", "Beta pública")))
                                    .child(el("td").child(l(locale, "Latest beta and main", "Última beta y main"))),
                            ),
                        ),
                )
                .child(el("p").child(l(
                    locale,
                    "After 1.0, this table will identify every maintained release line and end-of-support date.",
                    "Después de 1.0, esta tabla identificará cada línea mantenida y su fecha de fin de soporte.",
                ))),
        )
        .child(
            el("div")
                .class("rs-security-support__advisories")
                .child(el("span").class("utility-label").child("ADVISORIES / CURRENT"))
                .child(el("strong").child("00"))
                .child(el("h2").child(l(
                    locale,
                    "No published advisories.",
                    "Sin advisories publicados.",
                )))
                .child(el("p").child(l(
                    locale,
                    "No advisories are published as of 2026-07-16. This is a disclosure status, not a claim that undiscovered vulnerabilities do not exist.",
                    "No hay advisories publicados al 2026-07-16. Este es un estado de divulgación, no una afirmación de que no existan vulnerabilidades desconocidas.",
                )))
                .child(action(
                    "https://github.com/celiumsai/pliegors/security/advisories",
                    l(locale, "Open GitHub advisories", "Abrir advisories en GitHub"),
                    false,
                )),
        )
        .into_view()
}

fn security_report(locale: Locale) -> View {
    let steps = [
        (
            "01",
            l(locale, "Acknowledgment", "Confirmación"),
            l(
                locale,
                "Complete reports: within 3 business days",
                "Reportes completos: dentro de 3 días hábiles",
            ),
        ),
        (
            "02",
            l(locale, "Initial assessment", "Evaluación inicial"),
            l(
                locale,
                "Target: within 7 business days",
                "Objetivo: dentro de 7 días hábiles",
            ),
        ),
        (
            "03",
            l(locale, "Coordinated disclosure", "Divulgación coordinada"),
            l(
                locale,
                "Agree a date after a supported fix exists",
                "Acordar una fecha después de existir un fix soportado",
            ),
        ),
    ];
    let mut process = el("ol").class("rs-security-report__process");
    for (number, title, body) in steps {
        process = process.child(
            el("li")
                .child(el("span").child(number))
                .child(el("strong").child(title))
                .child(el("p").child(body)),
        );
    }
    el("section")
        .class("rs-security-report")
        .child(
            el("div")
                .class("rs-security-report__intro")
                .child(section_code(
                    "07",
                    l(locale, "Private disclosure", "Divulgación privada"),
                ))
                .child(el("h2").child(l(
                    locale,
                    "Found a boundary that does not hold? Tell us privately.",
                    "¿Encontraste un límite que no se sostiene? Repórtalo en privado.",
                )))
                .child(el("p").child(l(
                    locale,
                    "Include the version or commit, affected surface, minimal reproduction, impact, prerequisites, mitigations, and your disclosure preference. Never send unrelated credentials, private source, or personal data.",
                    "Incluye versión o commit, superficie afectada, reproducción mínima, impacto, prerrequisitos, mitigaciones y tu preferencia de divulgación. Nunca envíes credenciales, código privado o datos personales no relacionados.",
                )))
                .child(action(
                    "mailto:hello@pliegors.dev?subject=SECURITY%3A%20PliegoRS%20report",
                    "hello@pliegors.dev",
                    true,
                )),
        )
        .child(
            el("div")
                .class("rs-security-report__details")
                .child(process)
                .child(
                    el("div")
                        .class("rs-security-report__safe-harbor")
                        .child(el("strong").child(l(locale, "Good-faith research", "Investigación de buena fe")))
                        .child(el("p").child(l(
                            locale,
                            "Research that avoids privacy violations, data destruction, service degradation, and unauthorized access will be handled constructively. This policy never authorizes testing systems you do not own or have permission to test.",
                            "La investigación que evite violaciones de privacidad, destrucción de datos, degradación de servicio y acceso no autorizado será tratada constructivamente. Esta política nunca autoriza probar sistemas que no te pertenecen o para los que no tienes permiso.",
                        ))),
                )
                .child(
                    el("div")
                        .class("rs-security-report__links")
                        .child(
                            el("a")
                                .attr("href", "/.well-known/security.txt")
                                .child("/.well-known/security.txt")
                                .child(el("span").attr("aria-hidden", "true").child("↗")),
                        )
                        .child(
                            el("a")
                                .attr("href", "https://github.com/celiumsai/pliegors/blob/main/SECURITY.md")
                                .child(l(locale, "Repository security policy", "Política de seguridad del repositorio"))
                                .child(el("span").attr("aria-hidden", "true").child("↗")),
                        ),
                ),
        )
        .into_view()
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
                "Rules for using the website and public pre-1.0 software.",
                "Reglas de uso del sitio y del software público pre-1.0.",
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
                "These documents cover the public PliegoRS website, source, packages, and release materials.",
                "Estos documentos cubren el sitio público, el código, los paquetes y los materiales de release de PliegoRS.",
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
            "These terms govern access to the PliegoRS website and public release materials.",
            "Estos términos rigen el acceso al sitio de PliegoRS y a los materiales públicos de release.",
            vec![
                (
                    "01",
                    "Current status",
                    "Estado actual",
                    "PliegoRS 0.2.0-beta.1 is public pre-1.0 software. Documented APIs may change between prereleases or minor releases and changes are recorded in the changelog.",
                    "PliegoRS 0.2.0-beta.1 es software público pre-1.0. Las APIs documentadas pueden cambiar entre prereleases o releases menores y los cambios se registran en el changelog.",
                ),
                (
                    "02",
                    "License",
                    "Licencia",
                    "Framework source and packages are licensed under Apache-2.0. Brand assets and third-party files remain subject to their accompanying notices and policies.",
                    "El código y los paquetes del framework usan Apache-2.0. Los assets de marca y archivos de terceros conservan sus avisos y políticas aplicables.",
                ),
                (
                    "03",
                    "No warranty",
                    "Sin garantía",
                    "The software is provided without warranties to the extent permitted by law, as stated in the Apache-2.0 license.",
                    "El software se entrega sin garantías en la medida permitida por la ley, según la licencia Apache-2.0.",
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
