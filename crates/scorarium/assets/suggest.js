const DEBOUNCE_MS = 150;

function url(input) {
    const kind = input.dataset.suggest;
    const library = input.form?.dataset.libraryId;
    return library ? `/library/${library}/suggest/${kind}` : `/suggest/${kind}`;
}

function closeMenu() {
    document.querySelector("[data-suggest-menu]")?.remove();
}

function menuOf(input) {
    return input.parentElement.querySelector("[data-suggest-menu]");
}

function pick(input, match) {
    input.value = match.value;
    closeMenu();
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new CustomEvent("suggest:pick", { bubbles: true, detail: match }));
}

function openMenu(input, matches) {
    closeMenu();
    if (!matches.length) return;
    const menu = document.createElement("div");
    menu.className = "dropdown-menu show";
    menu.dataset.suggestMenu = "";
    // Spans the positioned parent: the input group when there is one, else the input's wrapper
    menu.style.cssText = "position: absolute; top: 100%; left: 0; right: 0; z-index: 1000";
    for (const match of matches) {
        const item = document.createElement("button");
        item.type = "button";
        item.className = "dropdown-item d-flex gap-3";
        const primary = document.createElement("span");
        primary.textContent = match.primary;
        if (match.exact) primary.className = "fw-bold";
        item.appendChild(primary);
        if (match.secondary) {
            const secondary = document.createElement("span");
            secondary.className = "text-body-secondary ms-auto text-truncate";
            secondary.textContent = match.secondary;
            item.appendChild(secondary);
        }
        if (match.reference?.source) {
            const badge = document.createElement("span");
            badge.className = "badge text-bg-secondary";
            badge.textContent = match.reference.source;
            item.appendChild(badge);
        }
        item.match = match;
        // Ahead of the input losing focus, so the click is not lost to the menu closing
        item.addEventListener("mousedown", (e) => {
            e.preventDefault();
            pick(input, match);
        });
        menu.appendChild(item);
    }
    input.parentElement.classList.add("position-relative");
    input.parentElement.appendChild(menu);
}

async function request(input) {
    const q = input.value.trim();
    const params = new URLSearchParams({ q });
    input.dispatchEvent(new CustomEvent("suggest:query", { bubbles: true, detail: { params } }));
    const sources = "suggestExternal" in input.dataset ? ["local", "external"] : ["local"];
    const results = new Map();
    for (const source of sources) {
        const query = new URLSearchParams(params);
        query.set("source", source);
        fetch(`${url(input)}?${query}`).then(async (response) => {
            if (!response.ok) return;
            const suggestions = await response.json();
            // A slow early response must not overwrite what a later keystroke asked for
            if (input.value.trim() !== q || document.activeElement !== input) return;
            results.set(source, suggestions);
            input.dispatchEvent(new CustomEvent("suggest:matches", { bubbles: true, detail: suggestions }));
            openMenu(
                input,
                sources.flatMap((s) => results.get(s)?.matches ?? []),
            );
        });
    }
}

document.addEventListener("input", (e) => {
    if (!e.isTrusted || !e.target.matches("[data-suggest]")) return;
    const input = e.target;
    const q = input.value.trim();
    // Every keystroke schedules a check; only the last one still sees its own text
    setTimeout(() => {
        if (input.value.trim() === q) request(input);
    }, DEBOUNCE_MS);
});

document.addEventListener("focusin", (e) => {
    if (e.target.matches("[data-suggest]")) request(e.target);
});

document.addEventListener("focusout", (e) => {
    if (e.target.matches("[data-suggest]")) closeMenu();
});

document.addEventListener(
    "keydown",
    (e) => {
        if (!e.target.matches("[data-suggest]")) return;
        const menu = menuOf(e.target);
        if (!menu) return;
        const items = [...menu.children];
        const active = menu.querySelector(".active");
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            const step = e.key === "ArrowDown" ? 1 : -1;
            const next = Math.min(Math.max(items.indexOf(active) + step, 0), items.length - 1);
            active?.classList.remove("active");
            items[next].classList.add("active");
        } else if (e.key === "Enter" && active) {
            // The form would otherwise submit on the way past
            e.preventDefault();
            pick(e.target, active.match);
        } else if (e.key === "Escape") {
            closeMenu();
        }
    },
    { capture: true },
);
