function attachListInput(container) {
    const name = container.dataset.listInput;
    const entries = container.querySelector("[data-entries]");
    const template = container.querySelector("template");
    const input = container.querySelector("[data-add]");

    const chips = "chips" in container.dataset;

    function add() {
        // The engine lowercases tags, so the chip shows what Save stores.
        const text = chips ? input.value.trim().toLowerCase() : input.value.trim();
        if (!text) {
            return;
        }
        if (!input.checkValidity()) {
            input.reportValidity();
            return;
        }
        if ([...entries.children].some((entry) => entry.dataset.value === text)) {
            return;
        }
        const entry = template.content.firstElementChild.cloneNode(true);
        entry.dataset.value = text;
        entry.querySelector("[data-label]").textContent = text;
        const favicon = entry.querySelector("[data-favicon]");
        if (favicon) {
            favicon.src = new URL(text).origin + "/favicon.ico";
        }
        entry.querySelector("input[type=hidden]").value = text;
        entries.append(entry);
        input.value = "";
        if (chips) {
            // Let the typeahead cancel its pending request and close its menu.
            input.dispatchEvent(new Event("input"));
        }
    }

    container.addEventListener("click", (event) => {
        const remove = event.target.closest("[data-remove]");
        if (remove) {
            const key = remove.dataset.key;
            if (key !== undefined) {
                const removal = document.createElement("input");
                removal.type = "hidden";
                removal.name = `remove_${name}`;
                removal.value = key;
                container.append(removal);
            }
            [...entries.children].find((entry) => entry.contains(remove)).remove();
        } else if (event.target.closest("[data-add-button]")) {
            add();
        }
    });

    input.addEventListener("keydown", (event) => {
        // A typeahead that picked a highlighted suggestion has already handled this Enter.
        if (event.key === "Enter" && !event.defaultPrevented) {
            event.preventDefault();
            add();
        }
    });

    if (chips) {
        container.addEventListener("click", (event) => {
            if (!event.target.closest("[data-remove]")) {
                input.focus();
            }
        });

        input.addEventListener("keydown", (event) => {
            if (event.key === " ") {
                event.preventDefault();
                add();
            } else if (event.key === "Backspace" && input.value === "") {
                entries.lastElementChild?.querySelector("[data-remove]")?.click();
            }
        });

        // Committing on blur lets a typed tag survive clicking Save without pressing Enter.
        input.addEventListener("blur", add);
        input.addEventListener("typeahead:pick", add);

        // A tag that is already a chip would be a no-op to add, so do not offer it.
        input.addEventListener("typeahead:suggest", () => {
            const menu = document.getElementById(input.getAttribute("aria-controls"));
            const present = new Set([...entries.children].map((entry) => entry.dataset.value.toLowerCase()));
            menu.querySelectorAll(".dropdown-item[data-value]").forEach((item) => {
                if (present.has(item.dataset.value.toLowerCase())) {
                    item.remove();
                }
            });
        });
    }
}

document.querySelectorAll("[data-list-input]").forEach(attachListInput);
