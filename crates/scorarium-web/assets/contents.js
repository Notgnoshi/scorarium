// While an existing work is picked in a contents entry form, disable the inputs that only describe
// a new work, since the server links the picked work as it is.
//
// Delegated on the document because htmx replaces the box after every change.
document.addEventListener("change", (event) => {
    const picked = event.target;
    const form = picked.closest?.("[data-contents-add]");
    if (!form || picked.type !== "hidden" || picked.name !== "work") {
        return;
    }
    const existing = picked.value !== "" && picked.value !== "new";
    form.querySelectorAll("input[data-new-work-only], [data-new-work-only] input").forEach(
        (input) => {
            input.disabled = existing;
        },
    );
});
