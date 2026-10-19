// Guidy site: theme toggle, generated bits of the mocks, the hero demo loop
// and scroll reveals. Motion stops when the OS asks for reduced motion.
(() => {
  const root = document.documentElement;
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => [...el.querySelectorAll(s)];

  // ── Theme: Guidy Dark / Guidy Light, following the OS until chosen.
  const modeBtn = $("#mode");
  const syncModeBtn = () => {
    const dark = root.dataset.mode !== "light";
    modeBtn.setAttribute("aria-label", dark ? "Switch to light mode" : "Switch to dark mode");
    $("use", modeBtn).setAttribute("href", dark ? "#i-sun" : "#i-moon");
  };
  modeBtn.addEventListener("click", () => {
    root.dataset.mode = root.dataset.mode === "light" ? "dark" : "light";
    localStorage.setItem("guidy-mode", root.dataset.mode);
    syncModeBtn();
  });
  matchMedia("(prefers-color-scheme: light)").addEventListener("change", (e) => {
    if (localStorage.getItem("guidy-mode")) return;
    root.dataset.mode = e.matches ? "light" : "dark";
    syncModeBtn();
  });
  syncModeBtn();

  // ── Pips (accuracy / speed out of five).
  $$("[data-pips]").forEach((el) => {
    const n = +el.dataset.pips;
    el.innerHTML = Array.from({ length: 5 }, (_, i) => `<i class="${i < n ? "f" : ""}"></i>`).join("");
    el.setAttribute("aria-label", `${n} of 5`);
  });

  // ── Overlay level meters.
  const WAVE = [6, 10, 16, 12, 18, 9, 14, 7, 5];
  $$("[data-wave]").forEach((el) => {
    el.innerHTML = WAVE.map((h, i) => `<i style="height:${h}px;animation-delay:${i * 0.08}s"></i>`).join("");
  });

  // ── Meeting talk-time lanes (same data as docs/design/guidy-mock.html).
  const talk = $("#talk");
  if (talk) {
    const sp = [
      ["You", true, [[2, 9], [22, 6], [40, 14], [71, 8]], "31%"],
      ["Priya", false, [[11, 10], [30, 9], [58, 12]], "28%"],
      ["Marcus", false, [[0, 2], [54, 4], [80, 15]], "22%"],
      ["Lena", false, [[17, 4], [86, 10]], "19%"],
    ];
    talk.innerHTML =
      sp.map(([n, a, segs, pct]) =>
        `<div class="tr ${a ? "a" : ""}"><span class="${a ? "acc" : ""}">${n}</span><div class="lane">${segs
          .map(([l, w]) => `<i class="seg-b" style="left:${l}%;width:${w}%"></i>`)
          .join("")}</div><span class="mono ${a ? "acc" : "muted"}" style="text-align:right">${pct}</span></div>`
      ).join("") +
      `<div class="axis"><span class="eyebrow">Min</span><div class="eyebrow"><span>0</span><span>10</span><span>20</span><span>30</span><span>42</span></div><span></span></div>`;
  }

  // ── Section name in the title bar ("Guidy / Models"), like the app.
  const where = $("#where");
  const named = $$("section[data-name]");
  const onScroll = () => {
    const y = scrollY + innerHeight * 0.35;
    const cur = named.filter((s) => s.offsetTop <= y).pop();
    where.textContent = cur ? cur.dataset.name : "Home";
    $$(".nav a").forEach((a) => a.classList.toggle("on", !!cur && a.getAttribute("href") === `#${cur.id}`));
  };
  addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  // ── Scroll reveals.
  const reveals = $$(".reveal");
  if (still || !("IntersectionObserver" in window)) {
    reveals.forEach((el) => el.classList.add("in"));
  } else {
    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -8% 0px" });
    reveals.forEach((el) => io.observe(el));
  }

  // ── Shortcut keycaps in "How it works" press in a slow loop.
  if (!still) {
    $$("[data-keys]").forEach((k) => {
      setInterval(() => { k.classList.add("down"); setTimeout(() => k.classList.remove("down"), 1400); }, 3200);
    });
  }

  // ── Hero demo: hold shortcut → live text in the overlay → paste.
  const typed = $("#typed"), live = $("#live-txt"), dock = $("#dock"), keys = $("#demo-keys"),
        timer = $("#timer"), state = $("#demo-state");
  if (!typed) return;

  const PHRASES = [
    "Let's move the review to Thursday and loop in the platform team so they can sign off on the token pipeline before we ship.",
    "Priya owns the model picker. Marcus will check first-run speed on older Intel machines.",
  ];

  if (still) {
    typed.textContent = PHRASES[0];
    return;
  }

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const fmt = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  async function run(phrase) {
    state.textContent = "Listening";
    keys.classList.add("down");
    live.innerHTML = "";
    timer.textContent = "0:00";
    dock.classList.remove("hidden");
    await sleep(500);

    // Streaming: words arrive a few at a time; the last two stay muted
    // until the model settles them.
    const words = phrase.split(" ");
    const t0 = Date.now();
    for (let i = 1; i <= words.length; i++) {
      const settled = words.slice(0, Math.max(0, i - 2)).join(" ");
      const tail = words.slice(Math.max(0, i - 2), i).join(" ");
      live.innerHTML = `${settled}${settled ? " " : ""}<span class="tail">${tail}</span>`;
      timer.textContent = fmt(Math.floor((Date.now() - t0) / 1000));
      await sleep(150 + Math.random() * 120);
    }
    live.textContent = phrase;
    await sleep(500);

    // Release: the overlay closes and the text is pasted at the caret.
    keys.classList.remove("down");
    state.textContent = "Pasted";
    dock.classList.add("hidden");
    typed.textContent += (typed.textContent ? " " : "") + phrase;
    await sleep(2600);
  }

  // Only animate while the demo is on screen.
  let visible = true;
  new IntersectionObserver(([e]) => { visible = e.isIntersecting; }).observe($(".demo"));

  (async () => {
    await sleep(900);
    for (;;) {
      for (const p of PHRASES) {
        while (!visible) await sleep(400);
        await run(p);
      }
      state.textContent = "Idle";
      await sleep(1200);
      typed.textContent = "";
    }
  })();
})();
