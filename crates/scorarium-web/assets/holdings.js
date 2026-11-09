{
    // An event from a detached element is dispatched on the document, which has no closest().
    const boxOf = (event) =>
        event.target instanceof Element ? event.target.closest("[data-holdings]") : null;

    // Collapsing hides the form without touching its state, and the form stays usable.
    const collapseAdd = (box) => {
        const form = box.querySelector("#holding-add-form");
        bootstrap.Collapse.getOrCreateInstance(form, { toggle: false }).hide();
    };

    const clone = (box, selector) =>
        box.querySelector(selector).content.firstElementChild.cloneNode(true);

    const entryOf = (box, id) => box.querySelector(`[data-transfer="${id}"]`);

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
        box.querySelector("#holding-entries").append(entry);
        collapseAdd(box);
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
            const parent = entry.parentElement;
            entry.outerHTML = event.detail.html;
            htmx.process(parent);
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

    document.addEventListener("htmx:after:request", (event) => {
        const box = boxOf(event);
        const added = box && event.target.matches("#holding-add-physical");
        if (added && event.detail.ctx.response.status < 400) {
            collapseAdd(box);
        }
    });

    document.addEventListener("click", (event) => {
        const dismiss = event.target.closest("[data-holdings] [data-transfer] [data-dismiss]");
        if (dismiss) {
            dismiss.closest("[data-transfer]").remove();
        }
    });
}
