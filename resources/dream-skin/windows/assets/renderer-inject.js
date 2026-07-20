((cssText, artDataUrl, theme) => {
  const STATE_KEY = "__CODEX_DREAM_SKIN_STATE__";
  const STYLE_ID = "codex-dream-skin-style";
  const CHROME_ID = "codex-dream-skin-chrome";
  const SKIN_VERSION = "1.1.5";
  const STYLE_VERSION = "6";
  const PINK_PLACEHOLDER = "随心输入，让灵感陪你一起写代码吧～";
  window.__CODEX_DREAM_SKIN_DISABLED__ = false;

  const escapeHtml = (value) => String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");

  const PINK_LAYOUT = Object.freeze({
    flowWidth: 1025,
    homeHeight: 797,
    heroWidth: 974,
    heroHeight: 511,
    flowTop: 45,
    flowGap: 22,
    copyTop: 172,
    copyLeft: 74,
    cardHeight: 184,
    cardFrameBottom: 14,
    cardFrameLeft: 35,
    cardFrameRight: 59,
    cardGridGap: 12,
    heroRadius: 18,
    signatureSize: 38,
    signatureLineHeight: 40,
    titleSize: 40,
    titleLineHeight: 49,
    subtitleSize: 16,
    subtitleLineHeight: 24.8,
    cardRadius: 16,
    cardHoverLift: -2,
    cardIconTop: 18,
    cardIconSize: 74,
    cardGlyphOne: 48,
    cardGlyphTwo: 45,
    cardGlyphThree: 44,
    cardTextTop: 98,
    cardTextInset: 12,
    cardTextHeight: 50,
    cardFontSize: 18,
    cardLineHeight: 25,
    cardHeartBottom: 16,
    cardHeartWidth: 18,
    cardHeartHeight: 16,
    composerWidth: 847,
    composerMinHeight: 141,
    projectHeight: 54,
    projectPaddingTop: 24,
    projectPaddingX: 6,
    projectPaddingBottom: 4,
    projectLabelLeft: 13,
    projectLabelTop: 6,
    projectLabelSize: 13,
    projectLabelLineHeight: 19.5,
    composerTopOffset: 69,
    railOffsetX: 22,
    panelRadius: 18,
    flowerWidth: 190,
    flowerHeight: 72,
    flowerBottom: 24,
    polaroidWidth: 108,
    polaroidHeight: 154,
    polaroidBorder: 7,
    polaroidBottomBorder: 48,
    polaroidLabelSize: 8,
    polaroidBowWidth: 30,
    polaroidBowHeight: 45,
    polaroidComposerGap: 24,
    polaroidBottomInset: 7,
  });
  const MANAGED_CLASSES = Object.freeze([
    "dream-home-flow",
    "dream-home-stage",
    "dream-hero",
    "dream-hero-copy",
    "dream-home-suggestions",
    "dream-suggestion-card",
    "dream-project-panel",
    "dream-home-composer",
    "dream-composer-rail",
    "dream-approval-mode",
    "dream-home-header",
    "dream-home-upsell",
    "dream-header-native-context",
  ]);

  const scaledCssLength = (value, scale) => {
    const rounded = Math.round(value * scale * 100) / 100;
    return `${rounded}px`;
  };

  const restoreManagedInlineStylesForNode = (node) => {
    if (!node) return;
    const properties = (node.dataset.dreamPinkInline || "").split(",").filter(Boolean);
    if (!properties.length) return;
    let ledger = {};
    try {
      ledger = JSON.parse(node.dataset.dreamPinkInlineOriginal || "{}");
    } catch {
      ledger = {};
    }
    properties.forEach((property) => {
      const record = ledger[property] || {};
      const hasManagedSignature = Object.prototype.hasOwnProperty.call(record, "managedValue");
      const stillManaged = !hasManagedSignature || (
        node.style.getPropertyValue(property) === record.managedValue &&
        node.style.getPropertyPriority(property) === (record.managedPriority || "important")
      );
      if (!stillManaged) return;
      node.style.removeProperty(property);
      if (record.value) node.style.setProperty(property, record.value, record.priority || "");
    });
    delete node.dataset.dreamPinkInline;
    delete node.dataset.dreamPinkInlineOriginal;
  };

  const clearManagedInlineStyles = () => {
    document.querySelectorAll("[data-dream-pink-inline]").forEach(restoreManagedInlineStylesForNode);
  };

  const setManagedInlineStyles = (node, styles) => {
    if (!node) return;
    restoreManagedInlineStylesForNode(node);
    const properties = [];
    const ledger = {};
    for (const [property, value] of Object.entries(styles)) {
      ledger[property] = {
        value: node.style.getPropertyValue(property),
        priority: node.style.getPropertyPriority(property),
      };
      node.style.setProperty(property, value, "important");
      ledger[property].managedValue = node.style.getPropertyValue(property);
      ledger[property].managedPriority = node.style.getPropertyPriority(property);
      properties.push(property);
    }
    node.dataset.dreamPinkInline = properties.join(",");
    node.dataset.dreamPinkInlineOriginal = JSON.stringify(ledger);
  };

  const updateManagedInlineStyle = (node, property, value) => {
    if (!node) return;
    const properties = (node.dataset.dreamPinkInline || "").split(",").filter(Boolean);
    if (!properties.includes(property)) {
      setManagedInlineStyles(node, { [property]: value });
      return;
    }
    node.style.setProperty(property, value, "important");
    let ledger = {};
    try {
      ledger = JSON.parse(node.dataset.dreamPinkInlineOriginal || "{}");
    } catch {
      ledger = {};
    }
    ledger[property] = {
      ...(ledger[property] || {}),
      managedValue: node.style.getPropertyValue(property),
      managedPriority: node.style.getPropertyPriority(property),
    };
    node.dataset.dreamPinkInlineOriginal = JSON.stringify(ledger);
  };

  const restoreManagedText = () => {
    document.querySelectorAll("[data-dream-placeholder-original]").forEach((node) => {
      const managed = node.dataset.dreamPlaceholderManaged;
      const current = node.getAttribute("data-placeholder");
      if (managed == null || current === managed) {
        if (node.dataset.dreamPlaceholderOriginalPresent === "false") {
          node.removeAttribute("data-placeholder");
        } else {
          node.setAttribute("data-placeholder", node.dataset.dreamPlaceholderOriginal || "");
        }
      }
      delete node.dataset.dreamPlaceholderOriginal;
      delete node.dataset.dreamPlaceholderOriginalPresent;
      delete node.dataset.dreamPlaceholderManaged;
    });
  };

  const previous = window[STATE_KEY];
  if (previous?.observer) previous.observer.disconnect();
  if (previous?.layoutObserver) previous.layoutObserver.disconnect();
  if (previous?.timer) clearInterval(previous.timer);
  if (previous?.scheduler?.timeout) clearTimeout(previous.scheduler.timeout);
  if (previous?.layoutScheduler?.frame != null) cancelAnimationFrame(previous.layoutScheduler.frame);
  if (previous?.resizeHandler) window.removeEventListener("resize", previous.resizeHandler);
  if (previous?.composerPointerHandler) {
    document.removeEventListener("pointerdown", previous.composerPointerHandler, true);
  }
  let previousArtUrl = previous?.artUrl || null;
  const artUrl = (() => {
    const comma = artDataUrl.indexOf(",");
    const mime = artDataUrl.slice(5, comma).split(";", 1)[0] || "image/png";
    const binary = atob(artDataUrl.slice(comma + 1));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return URL.createObjectURL(new Blob([bytes], { type: mime }));
  })();
  const existingStyle = document.getElementById(STYLE_ID);
  if (existingStyle) {
    existingStyle.textContent = cssText;
    existingStyle.dataset.dreamVersion = STYLE_VERSION;
  }

  let observeLayoutTargets = () => {};

  // When the responsive home composer is taller than its editor content,
  // delegate clicks in the remaining surface to the real editor. Keep native
  // controls (and direct editor clicks) on their normal event path.
  const composerPointerHandler = (event) => {
    if (theme?.id !== "pink-custom" || event.button !== 0) return;
    const target = event.target instanceof Element ? event.target : null;
    const composer = target?.closest(".dream-home-composer");
    if (!composer || target.closest([
      "button",
      "a",
      "input",
      "textarea",
      "select",
      "[role='button']",
      "[role='combobox']",
      "[contenteditable='true']",
    ].join(","))) return;
    const editor = composer.querySelector(".ProseMirror[contenteditable='true']");
    editor?.focus({ preventScroll: true });
  };
  document.addEventListener("pointerdown", composerPointerHandler, true);

  const ensure = () => {
    if (window.__CODEX_DREAM_SKIN_DISABLED__) return;
    const root = document.documentElement;
    if (!root) return;
    root.classList.add("codex-dream-skin");
    root.dataset.dreamTheme = theme?.id || "dream";
    if (theme?.galleryPreset === true) {
      root.dataset.dreamGallery = "true";
    } else {
      delete root.dataset.dreamGallery;
    }
    root.style.setProperty("--dream-art", `url("${artUrl}")`);
    if (previousArtUrl && previousArtUrl !== artUrl) {
      URL.revokeObjectURL(previousArtUrl);
      previousArtUrl = null;
    }

    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || root).appendChild(style);
    }
    if (style.dataset.dreamVersion !== STYLE_VERSION) {
      style.textContent = cssText;
      style.dataset.dreamVersion = STYLE_VERSION;
    }

    const shellMain = document.querySelector("main.main-surface") || document.querySelector("main");
    observeLayoutTargets(shellMain);
    const sidebar = document.querySelector("aside.app-shell-left-panel");
    const home = document.querySelector('[role="main"]:has([data-testid="home-icon"])');
    for (const candidate of document.querySelectorAll('[role="main"].dream-home')) {
      if (candidate !== home) candidate.classList.remove("dream-home");
    }
    if (home) home.classList.add("dream-home");

    for (const className of MANAGED_CLASSES) {
      document.querySelectorAll(`.${className}`).forEach((node) => node.classList.remove(className));
    }

    const homeFlow = home?.firstElementChild || null;
    const homeStage = homeFlow?.firstElementChild || null;
    const hero = homeStage?.firstElementChild || null;
    const heroCopy = hero?.querySelector('[data-feature="game-source"]') || null;
    let suggestions = home?.querySelector(".group\\/home-suggestions:not(.dream-fallback-suggestions)") || null;
    let fallbackFrame = hero?.querySelector(":scope > .dream-fallback-frame") || null;
    if (theme?.id === "pink-custom" && home && hero && !suggestions) {
      if (!fallbackFrame) {
        const labels = [
          "探索并理解代码",
          "构建新功能、应用或工具",
          "审查代码并提出修改建议",
          "修复问题和失败",
        ];
        fallbackFrame = document.createElement("div");
        fallbackFrame.className = "dream-fallback-frame";
        fallbackFrame.innerHTML = `<section class="group/home-suggestions dream-fallback-suggestions"><div><div>${labels.map((label) => `
          <div><button type="button" tabindex="-1"><span><span></span></span><span><span>${escapeHtml(label)}</span></span></button></div>`).join("")}
        </div></div></section>`;
        hero.appendChild(fallbackFrame);
      }
      suggestions = fallbackFrame.querySelector(".dream-fallback-suggestions");
    } else if (fallbackFrame) {
      fallbackFrame.remove();
      fallbackFrame = null;
    }
    const projectSelector = home?.querySelector(".group\\/project-selector") || null;
    const projectPanel = projectSelector?.closest("div:has(> .horizontal-scroll-fade-mask)") || null;
    const composer = home?.querySelector(".composer-surface-chrome") || null;
    const placeholder = composer?.querySelector("p[data-placeholder]") || null;
    let composerRail = composer?.parentElement || null;
    while (composerRail && composerRail !== home &&
      !composerRail.classList.contains("max-w-(--thread-content-max-width)")) {
      composerRail = composerRail.parentElement;
    }
    if (composerRail === home) composerRail = null;
    const homeHeader = shellMain?.querySelector(":scope > header.app-header-tint") || null;
    const headerSurface = homeHeader?.querySelector('[data-testid="app-shell-header-context-menu-surface"]') || null;
    const headerGrid = headerSurface?.firstElementChild?.firstElementChild || null;
    const nativeHeaderContext = headerGrid?.firstElementChild || null;

    homeFlow?.classList.add("dream-home-flow");
    homeStage?.classList.add("dream-home-stage");
    hero?.classList.add("dream-hero");
    heroCopy?.classList.add("dream-hero-copy");
    suggestions?.classList.add("dream-home-suggestions");
    suggestions?.querySelectorAll("button").forEach((button, index) => {
      button.classList.add("dream-suggestion-card");
      button.dataset.dreamCard = String(index + 1);
    });
    projectPanel?.classList.add("dream-project-panel");
    composer?.classList.add("dream-home-composer");
    composerRail?.classList.add("dream-composer-rail");
    composer?.querySelectorAll("button").forEach((button) => {
      if (button.matches('[data-composer-navigation-target="permissions"]') ||
          /替我审批|请求批准|完全访问|ask for approval|request approval|full access/i.test(button.textContent || "")) {
        button.classList.add("dream-approval-mode");
      }
    });
    if (home) homeHeader?.classList.add("dream-home-header");
    nativeHeaderContext?.classList.add("dream-header-native-context");
    if (home && homeHeader) {
      for (const control of homeHeader.querySelectorAll("button, a")) {
        const label = `${control.textContent || ""} ${control.getAttribute("aria-label") || ""}`;
        if (/\bplus\b|获取\s*plus|升级/i.test(label)) control.classList.add("dream-home-upsell");
      }
    }

    clearManagedInlineStyles();
    restoreManagedText();
    if (theme?.id === "pink-custom") {
      const sidebarBox = sidebar?.getBoundingClientRect() || null;
      const shellMainBox = shellMain?.getBoundingClientRect() || null;
      setManagedInlineStyles(homeHeader, {
        left: `${Math.round(sidebarBox?.right ?? shellMainBox?.left ?? 0)}px`,
        right: "0",
        width: "auto",
      });
      setManagedInlineStyles(nativeHeaderContext, {
        visibility: "hidden",
      });
    }
    if (theme?.id === "pink-custom" && hero) {
      const homeBox = home?.getBoundingClientRect() || null;
      const flowBox = homeFlow?.getBoundingClientRect() || null;
      const widthScale = flowBox?.width ? flowBox.width / PINK_LAYOUT.flowWidth : 1;
      const heightScale = homeBox?.height ? homeBox.height / PINK_LAYOUT.homeHeight : 1;
      const layoutScale = Math.max(.78, Math.min(2.4, widthScale, heightScale));
      const px = (value) => scaledCssLength(value, layoutScale);
      const suggestionFrame = suggestions?.closest(".dream-fallback-frame") || hero.children[1] || null;
      const suggestionViewport = suggestions?.firstElementChild || null;
      const suggestionGrid = suggestionViewport?.firstElementChild || null;
      root.dataset.dreamPinkScale = layoutScale.toFixed(4);
      setManagedInlineStyles(root, {
        "--pink-layout-scale": layoutScale.toFixed(4),
        "--pink-canvas-width": px(PINK_LAYOUT.heroWidth),
        "--pink-composer-width": px(PINK_LAYOUT.composerWidth),
        "--pink-flow-top": px(PINK_LAYOUT.flowTop),
        "--pink-flow-gap": px(PINK_LAYOUT.flowGap),
        "--pink-hero-height": px(PINK_LAYOUT.heroHeight),
        "--pink-hero-radius": px(PINK_LAYOUT.heroRadius),
        "--pink-signature-size": px(PINK_LAYOUT.signatureSize),
        "--pink-signature-line-height": px(PINK_LAYOUT.signatureLineHeight),
        "--pink-title-size": px(PINK_LAYOUT.titleSize),
        "--pink-title-line-height": px(PINK_LAYOUT.titleLineHeight),
        "--pink-subtitle-size": px(PINK_LAYOUT.subtitleSize),
        "--pink-subtitle-line-height": px(PINK_LAYOUT.subtitleLineHeight),
        "--pink-card-height": px(PINK_LAYOUT.cardHeight),
        "--pink-card-frame-bottom": px(PINK_LAYOUT.cardFrameBottom),
        "--pink-card-grid-gap": px(PINK_LAYOUT.cardGridGap),
        "--pink-card-radius": px(PINK_LAYOUT.cardRadius),
        "--pink-card-hover-lift": px(PINK_LAYOUT.cardHoverLift),
        "--pink-card-icon-top": px(PINK_LAYOUT.cardIconTop),
        "--pink-card-icon-size": px(PINK_LAYOUT.cardIconSize),
        "--pink-card-glyph-one": px(PINK_LAYOUT.cardGlyphOne),
        "--pink-card-glyph-two": px(PINK_LAYOUT.cardGlyphTwo),
        "--pink-card-glyph-three": px(PINK_LAYOUT.cardGlyphThree),
        "--pink-card-text-top": px(PINK_LAYOUT.cardTextTop),
        "--pink-card-text-inset": px(PINK_LAYOUT.cardTextInset),
        "--pink-card-text-height": px(PINK_LAYOUT.cardTextHeight),
        "--pink-card-font-size": px(PINK_LAYOUT.cardFontSize),
        "--pink-card-line-height": px(PINK_LAYOUT.cardLineHeight),
        "--pink-card-heart-bottom": px(PINK_LAYOUT.cardHeartBottom),
        "--pink-card-heart-width": px(PINK_LAYOUT.cardHeartWidth),
        "--pink-card-heart-height": px(PINK_LAYOUT.cardHeartHeight),
        "--pink-composer-min-height": px(PINK_LAYOUT.composerMinHeight),
        "--pink-project-height": px(PINK_LAYOUT.projectHeight),
        "--pink-project-label-left": px(PINK_LAYOUT.projectLabelLeft),
        "--pink-project-label-top": px(PINK_LAYOUT.projectLabelTop),
        "--pink-project-label-size": px(PINK_LAYOUT.projectLabelSize),
        "--pink-project-label-line-height": px(PINK_LAYOUT.projectLabelLineHeight),
        "--pink-panel-radius": px(PINK_LAYOUT.panelRadius),
        "--pink-flower-width": px(PINK_LAYOUT.flowerWidth),
        "--pink-flower-height": px(PINK_LAYOUT.flowerHeight),
        "--pink-flower-bottom": `-${px(PINK_LAYOUT.flowerBottom)}`,
        "--pink-polaroid-width": px(PINK_LAYOUT.polaroidWidth),
        "--pink-polaroid-height": px(PINK_LAYOUT.polaroidHeight),
        "--pink-polaroid-border": px(PINK_LAYOUT.polaroidBorder),
        "--pink-polaroid-bottom-border": px(PINK_LAYOUT.polaroidBottomBorder),
        "--pink-polaroid-label-size": px(PINK_LAYOUT.polaroidLabelSize),
        "--pink-polaroid-bow-width": px(PINK_LAYOUT.polaroidBowWidth),
        "--pink-polaroid-bow-height": px(PINK_LAYOUT.polaroidBowHeight),
      });
      setManagedInlineStyles(homeFlow, {
        "padding-top": px(PINK_LAYOUT.flowTop),
        "row-gap": px(PINK_LAYOUT.flowGap),
        "column-gap": px(PINK_LAYOUT.flowGap),
      });
      setManagedInlineStyles(hero, {
        width: px(PINK_LAYOUT.heroWidth),
        "max-width": px(PINK_LAYOUT.heroWidth),
        height: px(PINK_LAYOUT.heroHeight),
        "min-height": px(PINK_LAYOUT.heroHeight),
        "flex-grow": "0",
        "flex-shrink": "0",
        "flex-basis": "auto",
        overflow: "visible",
        border: "0",
        "background-image": "none",
      });
      // The reference artwork lets the girl's head rise above the hero's top
      // edge. One aligned artwork layer pair recreates it: .dream-hero-art
      // clips the panel render to the rounded frame (and carries its border),
      // .dream-hero-peek shows the same aligned render above the edge through
      // a soft silhouette mask. Both must stay AFTER the copy container and
      // card frame, which are addressed through :first-child / :nth-child(2).
      let heroArt = hero.querySelector(":scope > .dream-hero-art");
      if (!heroArt) {
        heroArt = document.createElement("div");
        heroArt.className = "dream-hero-art";
        heroArt.setAttribute("aria-hidden", "true");
        const heroArtImg = document.createElement("div");
        heroArtImg.className = "dream-hero-art-img";
        heroArt.append(heroArtImg);
      }
      hero.append(heroArt);
      let heroPeek = hero.querySelector(":scope > .dream-hero-peek");
      if (!heroPeek) {
        heroPeek = document.createElement("div");
        heroPeek.className = "dream-hero-peek";
        heroPeek.setAttribute("aria-hidden", "true");
      }
      hero.append(heroPeek);
      setManagedInlineStyles(hero.firstElementChild, {
        "align-items": "flex-start",
        "justify-content": "flex-start",
        "padding-top": px(PINK_LAYOUT.copyTop),
        "padding-right": "0",
        "padding-bottom": "0",
        "padding-left": px(PINK_LAYOUT.copyLeft),
      });
      setManagedInlineStyles(suggestionFrame, {
        position: "absolute",
        "z-index": "2",
        left: px(PINK_LAYOUT.cardFrameLeft),
        right: px(PINK_LAYOUT.cardFrameRight),
        width: "auto",
        top: "auto",
        bottom: px(PINK_LAYOUT.cardFrameBottom),
        height: px(PINK_LAYOUT.cardHeight),
        "margin-top": "0",
        "margin-right": "0",
        "margin-bottom": "0",
        "margin-left": "0",
      });
      setManagedInlineStyles(suggestions, { width: "100%", height: "100%", "align-items": "stretch" });
      setManagedInlineStyles(suggestionViewport, { width: "100%", height: "100%" });
      setManagedInlineStyles(suggestionGrid, { width: "100%", height: "100%", "align-items": "stretch" });
      suggestionGrid?.querySelectorAll(":scope > div").forEach((cardFrame) => {
        setManagedInlineStyles(cardFrame, { width: "100%", height: "100%", "min-width": "0" });
      });
      suggestions?.querySelectorAll("button").forEach((button) => {
        setManagedInlineStyles(button, { width: "100%", height: "100%", "min-width": "0", "min-height": "0" });
      });
      if (shellMain && composerRail) {
        const heroBox = hero.getBoundingClientRect();
        const scaledFlowBox = homeFlow?.getBoundingClientRect() || homeBox;
        const nativeRailBox = composerRail.getBoundingClientRect();
        const railWidth = Math.round(PINK_LAYOUT.composerWidth * layoutScale);
        const railLeft = Math.round(Math.max(0,
          heroBox.left - scaledFlowBox.left + (PINK_LAYOUT.railOffsetX * layoutScale)));
        const railShift = Math.round((heroBox.bottom + (10 * layoutScale)) - nativeRailBox.top);
        setManagedInlineStyles(composerRail, {
          position: "relative",
          width: `${railWidth}px`,
          "max-width": `${railWidth}px`,
          left: "auto",
          top: "auto",
          "margin-left": `${railLeft}px`,
          "margin-right": "auto",
          "padding-left": "0",
          "padding-right": "0",
          transform: `translateY(${railShift}px)`,
          "z-index": "6",
        });
        setManagedInlineStyles(projectPanel, {
          width: "100%",
          height: px(PINK_LAYOUT.projectHeight),
          "min-height": px(PINK_LAYOUT.projectHeight),
          "margin-top": "0",
          "margin-right": "0",
          "margin-bottom": "0",
          "margin-left": "0",
          top: "0",
          "padding-top": px(PINK_LAYOUT.projectPaddingTop),
          "padding-right": px(PINK_LAYOUT.projectPaddingX),
          "padding-bottom": px(PINK_LAYOUT.projectPaddingBottom),
          "padding-left": px(PINK_LAYOUT.projectPaddingX),
        });
        setManagedInlineStyles(composer, {
          width: "100%",
          "min-height": px(PINK_LAYOUT.composerMinHeight),
        });
        if (composer) {
          const desiredComposerTop = heroBox.bottom + (PINK_LAYOUT.composerTopOffset * layoutScale);
          const composerTop = composer.getBoundingClientRect().top;
          const correctedShift = Math.round(railShift + desiredComposerTop - composerTop);
          updateManagedInlineStyle(composerRail, "transform", `translateY(${correctedShift}px)`);
        }
      }
      if (placeholder) {
        placeholder.dataset.dreamPlaceholderOriginal = placeholder.getAttribute("data-placeholder") || "";
        placeholder.dataset.dreamPlaceholderOriginalPresent = String(placeholder.hasAttribute("data-placeholder"));
        placeholder.dataset.dreamPlaceholderManaged = PINK_PLACEHOLDER;
        placeholder.setAttribute("data-placeholder", PINK_PLACEHOLDER);
      }
    } else {
      delete root.dataset.dreamPinkScale;
    }

    if (!shellMain || !document.body) return;
    shellMain.classList.toggle("dream-home-shell", Boolean(home));
    let chrome = document.getElementById(CHROME_ID);
    if (!chrome || chrome.parentElement !== document.body) {
      chrome?.remove();
      chrome = document.createElement("div");
      chrome.id = CHROME_ID;
      chrome.setAttribute("aria-hidden", "true");
      chrome.innerHTML = `
        <div class="dream-brand"><span class="dream-note">♫</span><span><b>${escapeHtml(theme?.brandTitle || "Quotio Dream Skin")}</b><small>${escapeHtml(theme?.brandSubtitle || "CODEX THEME")} ✦</small></span></div>
        <div class="dream-signature">${escapeHtml(theme?.signature || theme?.name || "Dream ♡")}</div>
        <div class="dream-sparkles"><i></i><i></i><i></i><i></i><i></i><i></i></div>
        <div class="dream-ribbon"><span>♡</span>🎀<span>✦</span></div>
        <div class="dream-polaroid"><span class="dream-polaroid-tape"></span></div>`;
      document.body.appendChild(chrome);
    }
    const brandTitle = chrome.querySelector(".dream-brand b");
    const brandSubtitle = chrome.querySelector(".dream-brand small");
    const signature = chrome.querySelector(".dream-signature");
    if (brandTitle) brandTitle.textContent = theme?.brandTitle || "Quotio Dream Skin";
    if (brandSubtitle) brandSubtitle.textContent = `${theme?.brandSubtitle || "CODEX THEME"} ✦`;
    if (signature) signature.textContent = theme?.signature || theme?.name || "Dream ♡";
    const shellBox = shellMain.getBoundingClientRect();
    chrome.style.left = `${Math.round(shellBox.left)}px`;
    chrome.style.top = `${Math.round(shellBox.top)}px`;
    chrome.style.width = `${Math.round(shellBox.width)}px`;
    chrome.style.height = `${Math.round(shellBox.height)}px`;
    chrome.classList.toggle("dream-home-shell", Boolean(home));
    if (theme?.id === "pink-custom" && home && hero) {
      const heroBox = hero.getBoundingClientRect();
      const composerBox = composer?.getBoundingClientRect() || null;
      const brand = chrome.querySelector(".dream-brand");
      const polaroid = chrome.querySelector(".dream-polaroid");
      const scale = Number(root.dataset.dreamPinkScale || 1);
      if (polaroid && !polaroid.querySelector(".dream-polaroid-tape")) {
        const tape = document.createElement("span");
        tape.className = "dream-polaroid-tape";
        polaroid.appendChild(tape);
      }
      setManagedInlineStyles(brand, {
        left: `${Math.round(Math.max(18, heroBox.left - shellBox.left - 20))}px`,
        top: "3px",
      });
      setManagedInlineStyles(polaroid, {
        left: composerBox
          ? `${Math.round(composerBox.right - shellBox.left + (PINK_LAYOUT.polaroidComposerGap * scale))}px`
          : `${Math.round(heroBox.right - shellBox.left - (88 * scale))}px`,
        right: "auto",
        top: composerBox
          ? `${Math.round(composerBox.bottom - shellBox.top - ((PINK_LAYOUT.polaroidHeight + PINK_LAYOUT.polaroidBottomInset) * scale))}px`
          : "auto",
        bottom: "auto",
      });
    }
  };

  const cleanup = () => {
    window.__CODEX_DREAM_SKIN_DISABLED__ = true;
    document.documentElement?.classList.remove("codex-dream-skin");
    document.documentElement?.style.removeProperty("--dream-art");
    if (document.documentElement) {
      delete document.documentElement.dataset.dreamTheme;
      delete document.documentElement.dataset.dreamGallery;
      delete document.documentElement.dataset.dreamPinkScale;
    }
    document.querySelectorAll(".dream-home").forEach((node) => node.classList.remove("dream-home"));
    document.querySelectorAll(".dream-home-shell").forEach((node) => node.classList.remove("dream-home-shell"));
    for (const className of MANAGED_CLASSES) {
      document.querySelectorAll(`.${className}`).forEach((node) => node.classList.remove(className));
    }
    document.querySelectorAll("[data-dream-card]").forEach((node) => delete node.dataset.dreamCard);
    document.querySelectorAll(".dream-fallback-frame").forEach((node) => node.remove());
    document.querySelectorAll(".dream-hero-art").forEach((node) => node.remove());
    document.querySelectorAll(".dream-hero-peek").forEach((node) => node.remove());
    clearManagedInlineStyles();
    restoreManagedText();
    document.getElementById(STYLE_ID)?.remove();
    document.getElementById(CHROME_ID)?.remove();
    const state = window[STATE_KEY];
    state?.observer?.disconnect();
    state?.layoutObserver?.disconnect();
    if (state?.timer) clearInterval(state.timer);
    if (state?.scheduler?.timeout) clearTimeout(state.scheduler.timeout);
    if (state?.layoutScheduler?.frame != null) cancelAnimationFrame(state.layoutScheduler.frame);
    if (state?.resizeHandler) window.removeEventListener("resize", state.resizeHandler);
    if (state?.composerPointerHandler) {
      document.removeEventListener("pointerdown", state.composerPointerHandler, true);
    }
    if (state?.artUrl) URL.revokeObjectURL(state.artUrl);
    delete window[STATE_KEY];
    return true;
  };

  const scheduler = { timeout: null };
  const scheduleEnsure = () => {
    if (scheduler.timeout) clearTimeout(scheduler.timeout);
    scheduler.timeout = setTimeout(() => {
      scheduler.timeout = null;
      ensure();
    }, 180);
  };
  const layoutScheduler = { frame: null };
  const scheduleLayoutEnsure = () => {
    if (layoutScheduler.frame != null) return;
    layoutScheduler.frame = requestAnimationFrame(() => {
      layoutScheduler.frame = null;
      ensure();
    });
  };
  let observedShellMain = null;
  let observedSidebar = null;
  let observedHeader = null;
  const layoutObserver = typeof ResizeObserver === "function"
    ? new ResizeObserver(scheduleLayoutEnsure)
    : null;
  observeLayoutTargets = (shellMain = document.querySelector("main.main-surface") || document.querySelector("main")) => {
    if (!layoutObserver) return;
    const sidebar = document.querySelector("aside.app-shell-left-panel");
    const header = shellMain?.querySelector(":scope > header.app-header-tint") || null;
    if (shellMain === observedShellMain && sidebar === observedSidebar && header === observedHeader) return;
    layoutObserver.disconnect();
    observedShellMain = shellMain;
    observedSidebar = sidebar;
    observedHeader = header;
    if (observedShellMain) layoutObserver.observe(observedShellMain);
    if (observedSidebar) layoutObserver.observe(observedSidebar);
    if (observedHeader) layoutObserver.observe(observedHeader);
  };
  const observer = new MutationObserver(() => {
    observeLayoutTargets();
    scheduleEnsure();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  const timer = setInterval(ensure, 5000);
  const resizeHandler = scheduleLayoutEnsure;
  window.addEventListener("resize", resizeHandler, { passive: true });
  window[STATE_KEY] = {
    ensure,
    cleanup,
    observer,
    layoutObserver,
    timer,
    scheduler,
    layoutScheduler,
    resizeHandler,
    composerPointerHandler,
    artUrl,
    theme,
    version: SKIN_VERSION,
  };
  observeLayoutTargets();
  ensure();
  return { installed: true, version: SKIN_VERSION, theme: theme?.id || "dream" };
})(__DREAM_CSS_JSON__, __DREAM_ART_JSON__, __DREAM_THEME_JSON__)
