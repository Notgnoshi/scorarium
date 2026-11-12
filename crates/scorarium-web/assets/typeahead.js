function attachTypeahead(input) {
    if (input.dataset.typeaheadAttached) {
        return;
    }
    input.dataset.typeaheadAttached = "true";
    const menu = document.getElementById(input.getAttribute("aria-controls"));
    const picked = input.parentElement.querySelector("input[type=hidden]");
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

    function setPick(id) {
        picked.value = id;
        picked.dispatchEvent(new Event("change", { bubbles: true }));
        // An empty box is left to `required`, so an optional picker can still be submitted blank.
        const unpicked = !id && input.dataset.pickRequired && input.value.trim() !== "";
        input.setCustomValidity(unpicked ? input.dataset.pickRequired : "");
    }

    async function refresh(text) {
        // Aborting the older request keeps a slow response from overwriting a newer one.
        controller?.abort();
        controller = new AbortController();
        const params = new URLSearchParams({ q: text });
        const sibling = input.dataset.typeaheadWith;
        const value = sibling && input.form?.elements[sibling]?.value;
        if (value) {
            params.set(sibling, value);
        }
        let html;
        try {
            const response = await fetch(`${input.dataset.typeahead}?${params}`, {
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
        const offered = entries().length;
        input.dispatchEvent(new CustomEvent("typeahead:suggest", { bubbles: true }));
        // A listener that filtered out every suggestion leaves nothing to show.
        if (offered > 0 && entries().length === 0) {
            close();
        } else if (document.activeElement === input) {
            open();
            // The fragment lists its best match first, so Enter takes it without arrowing down,
            // unless the field opts out because its free text is usually the intended value.
            if (!("noDefaultPick" in input.dataset)) {
                highlight(entries()[0]);
            }
        }
    }

    input.addEventListener("input", () => {
        if (picked) {
            setPick("");
        }
        const text = input.value.trim();
        if (!text) {
            controller?.abort();
            close();
            return;
        }
        refresh(text);
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

    let windowLeft = false;
    let clickedIn = false;

    input.addEventListener("focus", () => {
        if (windowLeft) {
            windowLeft = false;
            return;
        }
        input.select();
        const text = input.value.trim();
        if (text) {
            refresh(text);
        }
    });

    input.addEventListener("mousedown", () => {
        clickedIn = document.activeElement !== input;
    });

    input.addEventListener("click", () => {
        if (clickedIn) {
            input.select();
        }
    });

    input.addEventListener("blur", () => {
        windowLeft = !document.hasFocus();
        close();
    });

    // Keep focus in the input until the entry's click fires, so blur does not close the menu first.
    menu.addEventListener("pointerdown", (event) => event.preventDefault());

    menu.addEventListener("click", (event) => {
        const entry = event.target.closest("[data-value]");
        if (entry) {
            input.value = entry.dataset.value;
            if (picked) {
                setPick(entry.dataset.pick ?? "");
            }
            input.dispatchEvent(new CustomEvent("typeahead:pick", { bubbles: true }));
            // A search still pending for the earlier text would open the menu again over the form.
            controller?.abort();
            close();
        }
    });

    if (picked) {
        setPick(picked.value);
    }
}

document.querySelectorAll("input[data-typeahead]").forEach(attachTypeahead);
// htmx swaps in fresh markup whose typeaheads need attaching. Settling fires once for each swapped
// piece, including out-of-band ones, which the swap event never reaches.
document.addEventListener("htmx:after:settle", (event) =>
    event.target.querySelectorAll("input[data-typeahead]").forEach(attachTypeahead),
);
// An appended list entry enters the page without htmx.
document.addEventListener("focusin", (event) => {
    if (event.target.matches?.("input[data-typeahead]")) {
        attachTypeahead(event.target);
    }
});
