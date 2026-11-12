{
    const serialize = (form) => new URLSearchParams(new FormData(form)).toString();

    // The snapshot is taken after every widget script has set up its fields.
    const snapshots = new Map();
    document.querySelectorAll("form[data-unsaved]").forEach((form) => snapshots.set(form, serialize(form)));

    // A form redrawn after a refused submit holds the refused edits, so its snapshot would match them.
    const changed = (form, snapshot) => "unsavedRefused" in form.dataset || serialize(form) !== snapshot;

    // Submitting the form, confirming a dialog, or discarding the changes is a decision about the
    // changes, so none of them asks again.
    let decided = false;

    // A form with no dialog id, such as the import form, leaves the asking to the browser.
    const edit = [...snapshots.keys()].find((form) => form.dataset.unsaved);
    const dialog = edit && document.getElementById(edit.dataset.unsaved);
    const save = dialog?.querySelector("[data-unsaved-save]");
    let proceed = null;

    // Save goes to the destination only when the server can redirect there with a GET.
    const offerSave = (target) => {
        save.hidden = target?.origin !== location.origin;
        if (!save.hidden) {
            const action = new URL(edit.action);
            action.searchParams.set("next", target.pathname + target.search);
            save.formAction = action.href;
        }
    };

    const ask = (event, target, go) => {
        if (!dialog || decided || !changed(edit, snapshots.get(edit))) {
            return;
        }
        event.preventDefault();
        proceed = go;
        offerSave(target);
        dialog.showModal();
    };

    // A modified click or a link with a target opens elsewhere, and a hash link stays on this page.
    const leaves = (event, link) =>
        event.button === 0 &&
        !(event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) &&
        !link.hasAttribute("target") &&
        !link.hasAttribute("download") &&
        link.href.split("#")[0] !== location.href.split("#")[0];

    const destination = (form, submitter) => {
        if (form.method !== "get") {
            return null;
        }
        const url = new URL(form.action);
        url.search = new URLSearchParams(new FormData(form, submitter)).toString();
        return url;
    };

    const disableSubmits = (form) =>
        // A button disabled during the event is not posted as the submitter.
        setTimeout(() => {
            for (const control of form.elements) {
                if (control.type === "submit" && !control.disabled) {
                    control.disabled = true;
                    control.dataset.submitted = "";
                }
            }
        });

    document.addEventListener("click", (event) => {
        const link = event.target.closest("a[href]");
        if (link && !event.defaultPrevented && leaves(event, link)) {
            // Cancel sits beside Save in the action bar, so its dialog has no save button.
            const target = link.matches("[data-cancel]") ? null : new URL(link.href);
            ask(event, target, () => {
                location.href = link.href;
            });
        }
    });

    document.addEventListener("submit", (event) => {
        const form = event.target;
        // A form that htmx sends does not leave the page, and has prevented the default by now.
        if (event.defaultPrevented || decided || form.method === "dialog") {
            return;
        }
        if (snapshots.has(form)) {
            decided = true;
            // A page restored from the history cache after this submit holds no unsaved changes.
            snapshots.set(form, serialize(form));
            delete form.dataset.unsavedRefused;
            disableSubmits(form);
        } else if (form.closest("dialog")) {
            // A form in a dialog, such as Delete's, has already asked.
            decided = true;
        } else {
            ask(event, destination(form, event.submitter), () => form.requestSubmit(event.submitter));
        }
    });

    dialog?.querySelector("[data-unsaved-discard]").addEventListener("click", () => {
        decided = true;
        dialog.close();
        proceed();
    });

    // The browser cannot show a validation message on a field behind a modal dialog, and a page
    // restored from the history cache should not reopen with the dialog showing.
    save?.addEventListener("click", () => dialog.close());

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

    // A page restored from the history cache has not been left.
    window.addEventListener("pageshow", (event) => {
        if (event.persisted) {
            decided = false;
            for (const control of document.querySelectorAll("[data-submitted]")) {
                control.disabled = false;
                delete control.dataset.submitted;
            }
        }
    });
}
