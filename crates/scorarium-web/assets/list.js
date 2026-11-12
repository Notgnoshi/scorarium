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

    document.addEventListener("click", (event) => {
        const add = event.target.closest("[data-list-add]");
        if (add) {
            append(add.closest("[data-list]"));
            return;
        }
        const entry = event.target.closest("[data-list-remove]")?.closest("[data-list-entry]");
        if (entry) {
            entry.closest("[data-list]").querySelector("[data-list-add]").focus();
            entry.dispatchEvent(new CustomEvent("list:remove", { bubbles: true }));
            entry.remove();
        }
    });
}
