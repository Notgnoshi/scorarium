// Show whether the contributor person box holds an existing person, a new one, or no pick yet.
// Delegated on the document because htmx replaces the section after every change.
document.addEventListener("change", (event) => {
    const picked = event.target;
    const container = picked.closest?.("[data-pick-icon]");
    if (!container || picked.type !== "hidden") {
        return;
    }
    const icon = container.querySelector(":scope > i");
    icon.classList.remove("bi-person-check", "bi-person-plus", "d-none");
    if (picked.value === "new") {
        icon.classList.add("bi-person-plus");
        icon.title = "New person";
    } else if (picked.value) {
        icon.classList.add("bi-person-check");
        icon.title = "Existing person";
    } else {
        icon.classList.add("d-none");
    }
});
