{
    const rootOf = (node) => node.closest("#review-queue");
    const keyOf = (root) => `review-queue-collapsed:${root.dataset.size}`;

    // A browser may refuse storage, in which case the toggles work for the page's lifetime only.
    const load = (root) => {
        try {
            return JSON.parse(sessionStorage.getItem(keyOf(root))) ?? {};
        } catch {
            return {};
        }
    };

    const save = (root, map) => {
        try {
            sessionStorage.setItem(keyOf(root), JSON.stringify(map));
        } catch {}
    };

    const entries = (root) => root.querySelectorAll("tbody[data-entry]");
    const togglable = (root) => [...entries(root)].filter((entry) => entry.querySelector("[data-toggle-entry]"));
    const anyExpanded = (root) => togglable(root).some((entry) => !entry.hasAttribute("data-collapsed"));

    const refreshButton = (root) => {
        const button = root.querySelector("[data-collapse-all]");
        if (button) {
            button.textContent = anyExpanded(root) ? "Collapse all" : "Expand all";
        }
    };

    const apply = () => {
        const root = document.getElementById("review-queue");
        if (!root) {
            return;
        }
        const map = load(root);
        for (const entry of entries(root)) {
            // An entry the user has not toggled should keep the state the server drew.
            if (entry.dataset.entry in map) {
                entry.toggleAttribute("data-collapsed", map[entry.dataset.entry] === true);
            }
        }
        refreshButton(root);
    };

    const set = (entry, collapsed, map) => {
        entry.toggleAttribute("data-collapsed", collapsed);
        map[entry.dataset.entry] = collapsed;
    };

    document.addEventListener("click", (event) => {
        const toggle = event.target.closest("[data-toggle-entry]");
        const all = event.target.closest("[data-collapse-all]");
        const root = rootOf(event.target);
        if (!root || !(toggle || all)) {
            return;
        }
        const map = load(root);
        if (toggle) {
            const entry = toggle.closest("tbody[data-entry]");
            set(entry, !entry.hasAttribute("data-collapsed"), map);
        } else {
            const collapse = anyExpanded(root);
            for (const entry of togglable(root)) {
                set(entry, collapse, map);
            }
        }
        save(root, map);
        refreshButton(root);
    });

    // A swap brings every entry back in the state the server drew, so the saved state is applied again.
    document.addEventListener("htmx:after:settle", apply);
    document.addEventListener("DOMContentLoaded", apply);
}
