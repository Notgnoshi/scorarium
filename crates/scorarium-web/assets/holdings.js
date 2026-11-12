{
    // An event from a detached element is dispatched on the document, which has no closest().
    const boxOf = (event) =>
        event.target instanceof Element ? event.target.closest("[data-holdings]") : null;

    const clone = (box, selector) =>
        box.querySelector(selector).content.firstElementChild.cloneNode(true);

    const entryOf = (box, id) => box.querySelector(`[data-transfer="${id}"]`);

    let nextAdded = 0;

    // The server draws the added entries of a resubmitted form with ids of the same shape, so an id
    // that is taken is skipped.
    const unusedLocationId = () => {
        let id;
        do {
            id = `holding-added-${nextAdded++}-location`;
        } while (document.getElementById(id));
        return id;
    };

    const add = (box) => {
        const field = box.querySelector("input[name=location]");
        const entry = clone(box, "template[data-staged-physical]");
        const input = entry.querySelector("input[data-typeahead]");
        const id = unusedLocationId();
        input.id = id;
        input.setAttribute("aria-controls", `${id}-menu`);
        input.value = field.value;
        entry.querySelector(".dropdown-menu").id = `${id}-menu`;
        box.querySelector("[data-entries]").append(entry);
        // The typeahead finds its menu by id, so it is attached once it is in the page.
        attachTypeahead(input);
        field.value = "";
        // Let the typeahead cancel its pending request and close its menu.
        field.dispatchEvent(new Event("input"));
    };

    const remove = (box, button) => {
        const entry = button.closest("[data-entries] > *");
        const { holding } = button.dataset;
        if (holding !== undefined) {
            const removal = document.createElement("input");
            removal.type = "hidden";
            removal.name = "remove_holding";
            removal.value = holding;
            box.append(removal);
        }
        // The staged file is useless once its entry is gone, so the server is told to forget it.
        const staged = entry.querySelector("input[data-staged]");
        if (staged) {
            fetch(`/api/stage/${encodeURIComponent(staged.value)}/discard`, {
                method: "POST",
                headers: { "HX-Current-URL": location.href },
            });
        }
        entry.remove();
    };

    document.addEventListener("upload:start", (event) => {
        const box = boxOf(event);
        if (!box) {
            return;
        }
        const { id, name, source } = event.detail;
        const entry = clone(box, "template[data-pending]");
        entry.dataset.transfer = id;
        // The end of the transfer does not repeat the name, which a failure must show.
        entry.dataset.name = name;
        if (source === "url") {
            entry.querySelector("[data-label]").textContent = `Fetching ${name}`;
            entry.querySelector("[data-progress]").hidden = true;
        } else {
            entry.querySelector("[data-label]").textContent = name;
            entry.querySelector("[data-spinner]").hidden = true;
        }
        box.querySelector("[data-entries]").append(entry);
    });

    document.addEventListener("upload:progress", (event) => {
        const box = boxOf(event);
        const entry = box && entryOf(box, event.detail.id);
        if (!entry) {
            return;
        }
        const percent = Math.floor((event.detail.loaded / event.detail.total) * 100);
        entry.querySelector("[data-progress]").setAttribute("aria-valuenow", percent);
        entry.querySelector(".progress-bar").style.width = `${percent}%`;
        entry.querySelector("[data-percent]").textContent = `${percent}%`;
    });

    document.addEventListener("upload:end", (event) => {
        const box = boxOf(event);
        const entry = box && entryOf(box, event.detail.id);
        if (!entry) {
            return;
        }
        if (event.detail.ok) {
            const { id, name } = JSON.parse(event.detail.body);
            const staged = clone(box, "template[data-staged-digital]");
            staged.querySelector("[data-label]").textContent = name;
            staged.querySelector("input[data-staged]").value = id;
            entry.replaceWith(staged);
            return;
        }
        const failed = clone(box, "template[data-failed]");
        failed.dataset.transfer = event.detail.id;
        // A refusal has no name, and a fetch's message often names its URL already, so the name
        // leads the message only when the message lacks it.
        const { name } = entry.dataset;
        const { message } = event.detail;
        const named = !name || message.includes(name);
        const text = named ? message : `${name}: ${message}`;
        failed.querySelector("[data-message]").textContent = text;
        entry.replaceWith(failed);
    });

    document.addEventListener("click", (event) => {
        const box = boxOf(event);
        if (!box) {
            return;
        }
        const dismiss = event.target.closest("[data-transfer] [data-dismiss]");
        const removeButton = event.target.closest("[data-remove]");
        if (dismiss) {
            dismiss.closest("[data-transfer]").remove();
        } else if (removeButton) {
            remove(box, removeButton);
        } else if (event.target.closest("[data-add-button]")) {
            add(box);
        }
    });

    document.addEventListener("keydown", (event) => {
        const box = boxOf(event);
        // A typeahead that picked a highlighted suggestion has already handled this Enter, and
        // Enter in any other field is left alone.
        const adding = box && event.target.matches("input[name=location]");
        if (adding && event.key === "Enter" && !event.defaultPrevented) {
            event.preventDefault();
            add(box);
        }
    });
}
