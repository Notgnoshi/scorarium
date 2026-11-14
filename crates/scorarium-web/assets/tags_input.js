function attachTagsInput(container) {
    const entries = container.querySelector("[data-entries]");
    const template = container.querySelector("template");
    const input = container.querySelector("[data-add]");

    function add() {
        // The engine lowercases tags, so the chip shows what Save stores.
        const value = input.value.trim().toLowerCase();
        if (!value) {
            return;
        }
        if (!input.checkValidity()) {
            input.reportValidity();
            return;
        }
        if ([...entries.children].some((entry) => entry.dataset.value === value)) {
            return;
        }
        const entry = template.content.firstElementChild.cloneNode(true);
        entry.dataset.value = value;
        entry.querySelector("[data-label]").textContent = value;
        entry.querySelector("input[type=hidden]").value = value;
        entries.append(entry);
        input.value = "";
        // Let the typeahead cancel its pending request and close its menu.
        input.dispatchEvent(new Event("input"));
    }

    container.addEventListener("click", (event) => {
        const remove = event.target.closest("[data-remove]");
        if (remove) {
            [...entries.children].find((entry) => entry.contains(remove)).remove();
        } else {
            input.focus();
        }
    });

    input.addEventListener("keydown", (event) => {
        // A typeahead that picked a highlighted suggestion has already handled this Enter.
        if ((event.key === "Enter" && !event.defaultPrevented) || event.key === " ") {
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

document.querySelectorAll("[data-tags-input]").forEach(attachTagsInput);
