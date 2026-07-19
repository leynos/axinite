/**
 * Mobile sidebar toggle behaviour.
 *
 * Expects these element ids in the page:
 *   #sidebar          – the aside element (hidden md:flex)
 *   #sidebar-toggle   – hamburger button (md:hidden)
 *   #sidebar-backdrop – semi-transparent overlay (hidden md:hidden)
 *   #sidebar-close    – optional close button inside the sidebar
 *
 * Call initSidebarToggle() once from the page script.
 * Returns { open, close, isOpen } for pages that need to hook the
 * sidebar state into other handlers (e.g. a global Escape listener).
 */
function initSidebarToggle() {
    var el       = document.getElementById('sidebar');
    var toggle   = document.getElementById('sidebar-toggle');
    var backdrop = document.getElementById('sidebar-backdrop');
    var close    = document.getElementById('sidebar-close');
    var isOpen   = false;
    var missing  = [];

    if (!el) missing.push('#sidebar');
    if (!toggle) missing.push('#sidebar-toggle');
    if (!backdrop) missing.push('#sidebar-backdrop');

    if (missing.length > 0) {
        console.warn('initSidebarToggle skipped: missing required element(s) ' + missing.join(', '));
        return {
            open: function () {},
            close: function () {},
            isOpen: function () { return false; }
        };
    }

    function open() {
        isOpen = true;
        el.classList.remove('hidden', 'relative');
        el.classList.add('flex', 'fixed', 'top-14', 'bottom-0', 'left-0', 'z-30');
        backdrop.classList.remove('hidden');
    }

    function shut() {
        isOpen = false;
        el.classList.remove('flex', 'fixed', 'top-14', 'bottom-0', 'left-0', 'z-30');
        el.classList.add('hidden', 'relative');
        backdrop.classList.add('hidden');
    }

    toggle.addEventListener('click', function () {
        if (isOpen) shut(); else open();
    });
    backdrop.addEventListener('click', shut);
    if (close) close.addEventListener('click', shut);

    window.addEventListener('resize', function () {
        if (window.innerWidth >= 768 && isOpen) shut();
    });

    document.addEventListener('keydown', function (e) {
        if (e.key === 'Escape' && isOpen) shut();
    });

    return {
        open: open,
        close: shut,
        isOpen: function () { return isOpen; }
    };
}
