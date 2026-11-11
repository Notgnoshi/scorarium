{
    let appended = 0;

    const append = (list) => {
        const template = list.querySelector(":scope > template");
        appended += 1;
        const blank = template.innerHTML.replaceAll("__key__", `new${appended}`);
        template.insertAdjacentHTML("beforebegin", blank);
        const entry = template.previousElementSibling;
        const field =
            entry.querySelector("[autofocus]") ??
            entry.querySelector("input:not([type=hidden]), select, textarea");
        field.focus();
    };

    const suffix = (field) => field.name.slice(field.name.indexOf("]") + 1);

    const sameField = (entry, field) =>
        [...entry.querySelectorAll("input")].find((other) => suffix(other) === suffix(field));

    document.addEventListener("keydown", (event) => {
        const field = event.target;
        const entry = field.closest?.("[data-list-enter] [data-list-entry]");
        if (event.key !== "Enter" || event.defaultPrevented || !entry) {
            return;
        }
        if (!field.matches("input:not([type=checkbox], [type=radio])")) {
            return;
        }
        event.preventDefault();
        let next = entry.nextElementSibling;
        while (next && !(next.matches("[data-list-entry]") && sameField(next, field))) {
            next = next.nextElementSibling;
        }
        if (next) {
            sameField(next, field).focus();
        } else if (field.value.trim() !== "") {
            append(entry.closest("[data-list]"));
        }
    });

    document.addEventListener("click", (event) => {
        const add = event.target.closest("[data-list-add]");
        if (add) {
            append(add.closest("[data-list]"));
            return;
        }
        const entry = event.target.closest("[data-list-remove]")?.closest("[data-list-entry]");
        if (entry) {
            entry.closest("[data-list]").querySelector("[data-list-add]").focus();
            entry.remove();
        }
    });
}
