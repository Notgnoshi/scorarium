// Show whether a picker holds an existing item, a new one, or no pick yet.
// Delegated on the document because htmx replaces the section after every change.
document.addEventListener("change", (event) => {
    const picked = event.target;
    const container = picked.closest?.("[data-pick-icon]");
    if (!container || picked.type !== "hidden") {
        return;
    }
    const icon = container.querySelector(":scope > i");
    const noun = container.dataset.pickIcon;
    const text = container.querySelector("input[data-typeahead]");
    // Typed text without a pick is a new item where a pick is not required.
    const typedNew = text?.value.trim() && !("pickRequired" in text.dataset);
    icon.classList.remove(container.dataset.existing, container.dataset.new, "d-none");
    if (picked.value === "new" || (!picked.value && typedNew)) {
        icon.classList.add(container.dataset.new);
        icon.title = `New ${noun}`;
    } else if (picked.value) {
        icon.classList.add(container.dataset.existing);
        icon.title = `Existing ${noun}`;
    } else {
        icon.classList.add("d-none");
    }
});
