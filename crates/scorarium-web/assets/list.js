{
    let appended = 0;

    const append = (list) => {
        const template = list.querySelector(":scope > template");
        const above = [...list.querySelectorAll(":scope > [data-list-entry]")].at(-1);
        appended += 1;
        const blank = template.innerHTML.replaceAll("__key__", `new${appended}`);
        template.insertAdjacentHTML("beforebegin", blank);
        const entry = template.previousElementSibling;
        const repeated = [];
        entry.querySelectorAll("[data-list-repeat] input").forEach((field) => {
            const source = above?.querySelector(`[name$="${field.name.match(/\[\w+\]$/)[0]}"]`);
            if (source) {
                field.value = source.value;
                repeated.push(field);
            }
        });
        repeated.forEach((field) => field.dispatchEvent(new Event("change", { bubbles: true })));
        const marked = [...entry.querySelectorAll("[autofocus]")];
        const field =
            marked.find((field) => field.value === "") ??
            marked[0] ??
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
