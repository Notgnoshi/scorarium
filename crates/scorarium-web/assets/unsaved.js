{
    const serialize = (form) => new URLSearchParams(new FormData(form)).toString();

    // The snapshot is taken after every widget script has set up its fields
    const snapshots = new Map();
    document.querySelectorAll("form[data-unsaved]").forEach((form) => snapshots.set(form, serialize(form)));

    // A form redrawn after a refused submit holds the refused edits, so its snapshot would match them.
    const changed = (form, snapshot) => "unsavedRefused" in form.dataset || serialize(form) !== snapshot;

    // Submitting the form, or cancelling it, is a decision about its changes, so neither asks for
    // confirmation. Any other form, such as a dialog's or the navbar search, decides nothing.
    let decided = false;

    document.addEventListener("submit", (event) => {
        // A form that htmx sends does not leave the page, and has prevented the default by now.
        if (!event.defaultPrevented && snapshots.has(event.target)) {
            decided = true;
        }
    });

    document.addEventListener("click", (event) => {
        // A modified click opens the link elsewhere and leaves this page as it is.
        const modified = event.ctrlKey || event.metaKey || event.shiftKey || event.altKey;
        if (!modified && event.target.closest("a[data-cancel]")) {
            decided = true;
        }
    });

    window.addEventListener("beforeunload", (event) => {
        if (decided) {
            return;
        }
        for (const [form, snapshot] of snapshots) {
            if (changed(form, snapshot)) {
                event.preventDefault();
                event.returnValue = true;
                return;
            }
        }
    });
}
