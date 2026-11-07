const DEBOUNCE_MS = 150;

function attachTypeahead(input) {
    const menu = input.nextElementSibling;
    let timer = null;
    let controller = null;

    const entries = () => [...menu.querySelectorAll(".dropdown-item")];
    const isOpen = () => menu.classList.contains("show");

    function highlight(entry) {
        menu.querySelector(".dropdown-item.active")?.classList.remove("active");
        if (entry) {
            entry.classList.add("active");
            entry.scrollIntoView({ block: "nearest" });
            input.setAttribute("aria-activedescendant", entry.id);
        } else {
            input.removeAttribute("aria-activedescendant");
        }
    }

    function open() {
        menu.classList.add("show");
        input.setAttribute("aria-expanded", "true");
    }

    function close() {
        highlight(null);
        menu.classList.remove("show");
        input.setAttribute("aria-expanded", "false");
    }

    async function refresh(text) {
        // Aborting the older request keeps a slow response from overwriting a newer one.
        controller?.abort();
        controller = new AbortController();
        let html;
        try {
            const response = await fetch(`${input.dataset.typeahead}?q=${encodeURIComponent(text)}`, {
                signal: controller.signal,
            });
            if (!response.ok) {
                return;
            }
            html = await response.text();
        } catch {
            return;
        }
        menu.innerHTML = html;
        entries().forEach((entry, i) => {
            entry.id = `${input.id}-option-${i}`;
        });
        if (document.activeElement === input) {
            open();
        }
    }

    input.addEventListener("input", () => {
        clearTimeout(timer);
        const text = input.value.trim();
        if (!text) {
            controller?.abort();
            close();
            return;
        }
        timer = setTimeout(() => refresh(text), DEBOUNCE_MS);
    });

    input.addEventListener("keydown", (event) => {
        switch (event.key) {
            case "ArrowDown":
            case "ArrowUp": {
                const all = entries();
                if (!isOpen() || all.length === 0) {
                    return;
                }
                event.preventDefault();
                const step = event.key === "ArrowDown" ? 1 : -1;
                const current = all.indexOf(menu.querySelector(".dropdown-item.active"));
                // From no highlight, Down goes to the first entry and Up to the last.
                const next = current === -1 ? (step === 1 ? 0 : all.length - 1) : current + step;
                highlight(all[(next + all.length) % all.length]);
                break;
            }
            case "Enter": {
                const active = isOpen() && menu.querySelector(".dropdown-item.active");
                if (active) {
                    event.preventDefault();
                    active.click();
                }
                break;
            }
            case "Escape":
                // Also stops a search input from clearing itself on Escape.
                event.preventDefault();
                if (isOpen()) {
                    close();
                } else {
                    input.blur();
                }
                break;
            case "Tab":
                close();
                break;
        }
    });

    input.addEventListener("blur", close);

    // Keep focus in the input until the entry's click fires, so blur does not close the menu first.
    menu.addEventListener("pointerdown", (event) => event.preventDefault());

    menu.addEventListener("click", (event) => {
        const entry = event.target.closest("[data-value]");
        if (entry) {
            input.value = entry.dataset.value;
            close();
        }
    });
}

document.querySelectorAll("input[data-typeahead]").forEach(attachTypeahead);
