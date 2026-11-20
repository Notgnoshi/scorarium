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

document.addEventListener("DOMContentLoaded", () => {
    document.querySelector("dialog[data-open]")?.showModal();
});

document.addEventListener("htmx:after:settle", (event) => {
    if (event.target.matches("[data-dialog-slot]")) {
        event.target.querySelector("dialog")?.showModal();
    }
});

// An open modal dialog keeps the cursor out of an answer that is drawn elsewhere.
document.addEventListener("htmx:before:swap", (event) => {
    if (event.detail.ctx.hx?.retarget) {
        event.target.closest?.("[data-dialog-slot] dialog")?.remove();
    }
});

// A bare key is a shortcut only when the user is not typing, not using a modifier, and not in a dialog.
function isShortcut(event) {
    if (event.ctrlKey || event.metaKey || event.altKey) {
        return false;
    }
    const target = event.target;
    if (target.matches("input, textarea, select") || target.isContentEditable) {
        return false;
    }
    return !document.querySelector("dialog[open]");
}

// Focus the navbar search box on "/".
document.addEventListener("keydown", (event) => {
    if (event.key !== "/" || !isShortcut(event)) return;
    const search = document.getElementById("q");
    if (search) {
        event.preventDefault();
        search.focus();
        search.select();
    }
});

document.addEventListener("keydown", (event) => {
    if (!isShortcut(event)) return;
    const element = document.querySelector(`[data-hotkey="${CSS.escape(event.key)}"]`);
    if (element) {
        event.preventDefault();
        element.click();
    }
});
