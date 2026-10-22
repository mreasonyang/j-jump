(() => {
  "use strict";
  const zh = {
    skip: "跳到正文",
    navHow: "工作方式",
    navCommands: "常用命令",
    navInstall: "安装",
    release: "v0.0.47 · 开源 Beta",
    heroLine1: "少记路径。",
    heroLine2: "专注当下。",
    heroDescription:
      "跳到你去过的目录。几个字母、模糊词义，甚至换一种语言，全在你的 Shell 命令行中完成。",
    getStarted: "快速安装",
    seeHow: "查看演示",
    localMode: "本地匹配 · 无需联网",
    replay: "重置",
    byName: "按名称",
    byMeaning: "按含义",
    byLanguage: "跨语言",
    demoCaption: "点选一个场景。这是交互示意，不会执行命令。",
    principle1: "本地优先。",
    principle2: "语义按需开启。",
    principle3: "语义建议，由你选择。",
    howEyebrow: "找到熟悉的方向",
    howTitle1: "你记得要做的事。",
    howTitle2: "它记得该走的路。",
    howDescription:
      "照常使用 cd。J-Jump 记下你去过的目录，让下一次访问少打几个字。",
    nameTitle: "记得一点名字，就够了。",
    nameDescription:
      "输入几个字母，从访问过的目录里按名称匹配，再结合访问频率和最近访问时间排序。整个过程在本机完成。",
    meaningTitle: "忘了名字，就说含义。",
    meaningDescription:
      "可选的语义模型会从你访问过的目录里建议一个匹配项。模型提供建议，你来决定去哪里。",
    languageTitle: "用你习惯的语言直接搜。",
    languageDescription:
      "思考的语言，不必与目录名一致。借助你选择的模型，用含义寻找目标，也可以跨越语言。",
    controlEyebrow: "一点帮助，决定权在你",
    controlTitle1: "你的终端。",
    controlTitle2: "你的规则。",
    controlIntro:
      "从本地跳转开始，有需要再开启语义功能。模型给出建议，最终由你选择。",
    readPrivacy: "查看配置与隐私说明",
    offlineTitle: "没有模型，也能用。",
    offlineDescription:
      "名称匹配和访问记录保留在本机。本地导航无需账号、API Key 或网络。",
    providerTitle: "模型在哪里跑，你来选。",
    providerDescription:
      "可以选择云端 Jev、Cloudflare Clef-Flash、OpenAI Decisions，也可以用 Ollama 在本机运行 Tev1。",
    privacyTitle: "清楚掌控，哪些数据离开本机。",
    privacyDescription:
      "语义功能默认关闭。严格模式仅发送查询词与候选目录名，不会向模型发送文件内容、代码或环境变量。你可随时预览请求并排除敏感目录。",
    commandsEyebrow: "简单命令，日常使用",
    commandsTitle1: "融入你的习惯。",
    commandsTitle2: "从不打扰。",
    commandsDescription:
      "几个小命令，让目录间的移动更自然。原来的 cd 依然照常工作。",
    fullDocs: "阅读完整指南",
    cmd1: "沿路径按顺序匹配多个词。",
    cmd2: "只在当前目录之下查找已访问的目录。",
    cmd3: "返回上一个目录。",
    cmd4: "从访问记录中选择目录。",
    cmd5: "补全路径到命令行，按回车才跳转。",
    cmd6: "离线检查配置，给出修复建议。",
    installEyebrow: "随时可以出发",
    installTitle: "装好即用，重回你的节奏。",
    installDescription: "安装一次，打开新终端。离下一个想法，再近一步。",
    installScript: "安装脚本",
    allReleases: "所有版本",
    copy: "复制",
    brewNote: "安装 J-Jump，并接入你的 shell。",
    scriptNote:
      "验证 SHA-256 后安装到 ~/.local/bin，无需 sudo；同时接入 shell。",
    step1: "完成安装后，打开一个新终端。",
    step2: "照常使用 cd，积累访问记录。",
    step3: "运行 j 或 ji；首次向导选择 off 即可保持纯本地，也可按需挑选语义服务。",
    installPlatforms:
      "macOS 15+ · Linux · Apple silicon / Intel / ARM64 / x86-64 · Bash / Zsh / Fish",
    faqEyebrow: "你可能想知道",
    faqTitle: "出发之前。",
    faq1q: "一定要用 AI 吗？",
    faq1a:
      "不用。名称匹配、访问记录和导航都可以在本机完成。首次向导选择 off 即可；以后可通过 jjump setup 开启语义服务。",
    faq2q: "它会搜索我的整个电脑吗？",
    faq2a:
      "不会。J-Jump 只搜索你安装之后访问过的目录，初始记录为空。先用 cd 到一个目录，再用 j 或 ji 找到它。",
    faq3q: "免费吗？",
    faq3a:
      "J-Jump 以 MIT 协议免费开源。可选的云端模型服务可能按 API 请求收费。本地 Tev1 需要自行安装 Ollama 并下载模型。",
    faq4q: "我已经在用 j 这个命令，怎么办？",
    faq4a: "接入 shell 时可以换一个名字：",
    faq4b: "，之后就可以使用 jump 和 jumpi。",
    faq5q: "使用 Beta 版本需要了解什么？",
    faq5a:
      "J-Jump 仍处于 0.0.x 早期版本，可能遇到不完善之处。部分升级需要一次性修复配置或历史数据，jjump doctor 会给出指引。发布二进制尚未签名，安装器通过 SHA-256 校验完整性。",
    closing: "好工具，为真正的工作留出空间。",
    feedback: "一起打磨 J-Jump",
    footerNote: "用 Rust 构建，以 MIT 协议分享。",
    license: "许可协议",
  };
  const en = Object.fromEntries(
    [...document.querySelectorAll("[data-i18n]")].map((el) => [
      el.dataset.i18n,
      el.textContent,
    ]),
  );
  en.scriptNote =
    "Checks SHA-256, installs to ~/.local/bin without sudo, and connects your shell.";
  const translations = { en, zh };
  let lang = "en";
  try {
    lang = localStorage.getItem("jjump-site-language") === "zh" ? "zh" : "en";
  } catch {
    /* Storage is optional. */
  }
  let scenario = "name";
  let chosen = false;
  let installMethod = "brew";
  let toastTimer;
  const commands = {
    brew: "brew install mreasonyang/taps/j-jump && jjump shell install",
    script:
      "curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh",
  };
  const scenarios = {
    name: { command: "j pay", path: "~/work/payments" },
    meaning: { command: "ji my cv", path: "~/documents/resumes" },
    language: { command: "ji 税务", path: "~/documents/taxes" },
  };
  const t = (key) => translations[lang][key] || en[key];
  function renderDemo() {
    const data = scenarios[scenario];
    const semantic = scenario !== "name";
    document.querySelector("#demo-command").textContent = data.command;
    document
      .querySelectorAll("[data-demo]")
      .forEach((button) =>
        button.setAttribute(
          "aria-pressed",
          String(button.dataset.demo === scenario),
        ),
      );
    const result = document.querySelector("#demo-result");
    result.replaceChildren();
    const note = document.createElement("p");
    note.className = "result-note";
    note.textContent = semantic
      ? lang === "zh"
        ? "请求已确认 · 以下为模型建议"
        : "Request approved · suggested by your model"
      : lang === "zh"
        ? "在访问记录中找到匹配目录"
        : "Found in your visited folders";
    result.append(note);
    if (semantic && !chosen) {
      const pick = document.createElement("button");
      pick.type = "button";
      pick.className = "demo-pick";
      const number = document.createElement("span");
      number.textContent = "1";
      const path = document.createElement("span");
      path.textContent = data.path;
      const action = document.createElement("span");
      action.textContent = lang === "zh" ? "选择 ↵" : "Choose ↵";
      pick.append(number, path, action);
      pick.addEventListener("click", () => {
        chosen = true;
        renderDemo();
        const resultPath = document.querySelector(".result-path");
        resultPath.tabIndex = -1;
        resultPath.focus({ preventScroll: true });
      });
      result.append(pick);
      const hint = document.createElement("p");
      hint.className = "result-hint";
      hint.textContent =
        lang === "zh"
          ? "还未跳转。点击建议，确认你的选择。"
          : "Still in ~/work. Select the suggestion to jump.";
      result.append(hint);
    } else {
      const path = document.createElement("div");
      path.className = "result-path";
      const check = document.createElement("span");
      check.className = "result-check";
      check.textContent = "✓";
      path.append(check, document.createTextNode(data.path));
      result.append(path);
      const prompt = document.createElement("p");
      prompt.className = "terminal-new-prompt";
      const arrow = document.createElement("span");
      arrow.textContent = "❯";
      prompt.append(
        arrow,
        document.createTextNode(
          lang === "zh"
            ? "到达。继续你的工作。"
            : "You’re here. Back to your work.",
        ),
      );
      result.append(prompt);
    }
    document.querySelector("#demo-mode").replaceChildren();
    const dot = document.createElement("i");
    const label = document.createElement("span");
    label.textContent = semantic
      ? lang === "zh"
        ? "语义建议 · 由你选择"
        : "SEMANTIC · YOU CHOOSE"
      : t("localMode");
    document.querySelector("#demo-mode").append(dot, label);
  }
  function renderInstall() {
    document.querySelector("#install-code").textContent =
      commands[installMethod];
    document.querySelector("#install-note").textContent = t(
      installMethod === "brew" ? "brewNote" : "scriptNote",
    );
    document.querySelector("#copy-label").textContent = t("copy");
    document
      .querySelectorAll("[data-install]")
      .forEach((button) =>
        button.setAttribute(
          "aria-pressed",
          String(button.dataset.install === installMethod),
        ),
      );
  }
  function setLanguage(next) {
    lang = next;
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
    const ariaLabels = {
      ".site-header .brand": ["J-Jump home", "J-Jump 首页"],
      ".site-header nav": ["Main navigation", "主导航"],
      ".terminal": [
        "Interactive illustrative terminal demo",
        "终端交互示意演示",
      ],
      ".demo-tabs": ["Choose a demo", "选择演示场景"],
      ".install-tabs": ["Installation method", "安装方式"],
    };
    for (const [selector, labels] of Object.entries(ariaLabels))
      document
        .querySelector(selector)
        .setAttribute("aria-label", labels[lang === "zh" ? 1 : 0]);
    document.querySelectorAll("[data-i18n]").forEach((el) => {
      const value = t(el.dataset.i18n);
      if (value) el.textContent = value;
    });
    const button = document.querySelector("#language-toggle");
    button.textContent = lang === "zh" ? "EN ↔" : "中文 ↔";
    button.setAttribute(
      "aria-label",
      lang === "zh" ? "Switch to English" : "切换到中文",
    );
    document
      .querySelectorAll("[data-doc-link]")
      .forEach(
        (a) =>
          (a.href =
            "https://github.com/mreasonyang/j-jump/blob/main/docs/" +
            (lang === "zh" ? "CONFIGURATION-AND-HELP.md" : "CONFIGURATION.md")),
      );
    document.title =
      lang === "zh"
        ? "J-Jump — 少记路径，专注当下。"
        : "J-Jump — Less path. More flow.";
    document.querySelector('meta[name="description"]').content =
      lang === "zh"
        ? "用名称、含义或另一种语言，跳到你访问过的目录。J-Jump 是面向 macOS 和 Linux 的 Rust 终端工具，支持 Bash、Zsh 和 Fish。"
        : "A small Rust tool for jumping to the folders you visit. Find them by name, by meaning, or across languages. For Bash, Zsh and Fish on macOS and Linux.";
    renderDemo();
    renderInstall();
    try {
      localStorage.setItem("jjump-site-language", lang);
    } catch {
      /* Storage is optional. */
    }
  }
  document
    .querySelector("#language-toggle")
    .addEventListener("click", () => setLanguage(lang === "en" ? "zh" : "en"));
  document.querySelectorAll("[data-demo]").forEach((button) =>
    button.addEventListener("click", () => {
      scenario = button.dataset.demo;
      chosen = false;
      renderDemo();
    }),
  );
  document.querySelector("#demo-replay").addEventListener("click", () => {
    chosen = false;
    renderDemo();
  });
  document.querySelectorAll("[data-install]").forEach((button) =>
    button.addEventListener("click", () => {
      installMethod = button.dataset.install;
      renderInstall();
    }),
  );
  document
    .querySelector("#copy-install")
    .addEventListener("click", async () => {
      let copied = false;
      try {
        await navigator.clipboard.writeText(commands[installMethod]);
        copied = true;
      } catch {
        const selection = window.getSelection();
        const range = document.createRange();
        range.selectNodeContents(document.querySelector("#install-code"));
        selection.removeAllRanges();
        selection.addRange(range);
        try {
          copied = document.execCommand("copy");
        } catch {
          /* Keep the command selected for manual copy. */
        }
        if (copied) selection.removeAllRanges();
      }
      const message = copied
        ? lang === "zh"
          ? "安装命令已复制"
          : "Install command copied"
        : lang === "zh"
          ? "命令已选中，请按 ⌘C 或 Ctrl+C 复制"
          : "Command selected. Press ⌘C or Ctrl+C to copy.";
      document.querySelector("#copy-label").textContent = copied
        ? lang === "zh"
          ? "已复制"
          : "Copied"
        : t("copy");
      const toast = document.querySelector("#toast");
      toast.textContent = message;
      toast.hidden = false;
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => {
        toast.hidden = true;
        document.querySelector("#copy-label").textContent = t("copy");
      }, 3000);
    });
  setLanguage(lang);
})();
