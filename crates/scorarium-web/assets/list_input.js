function attachListInput(container) {
    const name = container.dataset.listInput;
    const entries = container.querySelector("[data-entries]");
    const template = container.querySelector("template");
    const input = container.querySelector("[data-add]");

    function add() {
        const text = input.value.trim();
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
        entry.querySelector("input[type=hidden]").value = text;
        entries.append(entry);
        input.value = "";
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
}

document.querySelectorAll("[data-list-input]").forEach(attachListInput);
