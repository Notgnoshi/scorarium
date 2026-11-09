const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function applyTheme() {
    document.documentElement.setAttribute("data-bs-theme", darkQuery.matches ? "dark" : "light");
}

applyTheme();
darkQuery.addEventListener("change", applyTheme);

// Delegated on the document so the listener works from <head> and covers every button that opts in.
document.addEventListener("click", (event) => {
    const button = event.target.closest("[data-dialog]");
    if (button) {
        document.getElementById(button.dataset.dialog).showModal();
    }
});

// A dialog that htmx swaps into a slot opens itself. The slot is the target of the settle event.
document.addEventListener("htmx:after:settle", (event) => {
    if (event.target.matches("[data-dialog-slot]")) {
        event.target.querySelector("dialog")?.showModal();
    }
});

// Focus the navbar search box on "/", unless the slash is being typed into something editable.
document.addEventListener("keydown", (event) => {
    if (event.key !== "/" || event.ctrlKey || event.metaKey || event.altKey) {
        return;
    }
    const target = event.target;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
        return;
    }
    const search = document.getElementById("q");
    if (search) {
        event.preventDefault();
        search.focus();
        search.select();
    }
});
