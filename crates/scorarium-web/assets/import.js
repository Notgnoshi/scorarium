{
    const field = document.getElementById("text");
    const record = field?.form?.elements.record;
    const summary = document.getElementById("candidate-summary");
    const content = document.getElementById("candidate-content");
    const link = document.getElementById("candidate-link");

    if (field?.dataset.typeahead && record && summary) {
        field.addEventListener("typeahead:pick", () => {
            const menu = document.getElementById(field.getAttribute("aria-controls"));
            const pick = [...menu.querySelectorAll("[data-pick]")].find(
                (entry) => entry.dataset.pick === record.value,
            );
            if (!pick) {
                return;
            }
            // The suggestion's content was escaped by the server, so cloning it is safe.
            content.replaceChildren(
                ...[...pick.childNodes].map((node) => node.cloneNode(true)),
            );
            link.href = record.value;
            summary.hidden = false;
        });

        field.addEventListener("input", () => {
            summary.hidden = true;
            content.replaceChildren();
        });
    }
}
