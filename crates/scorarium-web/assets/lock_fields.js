// Disable the marked fields of a list entry while its marked picker holds an existing item.
document.addEventListener("change", (event) => {
    const picked = event.target;
    if (picked.type !== "hidden" || !picked.closest?.("[data-locks-fields]")) {
        return;
    }
    const locked = picked.value !== "" && picked.value !== "new";
    picked
        .closest("[data-list-entry]")
        .querySelectorAll("[data-lockable] input")
        .forEach((field) => {
            field.disabled = locked;
        });
});
