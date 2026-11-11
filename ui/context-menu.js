function reportActionError(error) {
  const notice = $('action-error');
  notice.textContent = String(error?.message ?? error);
  clearTimeout(reportActionError.timer);
  reportActionError.timer = setTimeout(() => { notice.textContent = ''; }, 6000);
}

async function runAction(action) {
  try {
    await action();
  } catch (error) {
    reportActionError(error);
  }
}

async function copyText(value) {
  await navigator.clipboard.writeText(value);
}

const ContextMenu = (() => {
  const root = $('context-menu');
  let submenu;
  let trigger;
  let restoreFocus;
  let updateItems;

  function position(menu, x, y) {
    const gap = 6;
    menu.style.left = `${Math.max(gap, Math.min(x, innerWidth - menu.offsetWidth - gap))}px`;
    menu.style.top = `${Math.max(gap, Math.min(y, innerHeight - menu.offsetHeight - gap))}px`;
  }

  function closeSubmenu() {
    submenu?.remove();
    trigger?.setAttribute('aria-expanded', 'false');
    submenu = trigger = null;
  }

  function close(restore = false) {
    closeSubmenu();
    root.hidden = true;
    root.replaceChildren();
    if (restore) restoreFocus?.();
    restoreFocus = null;
    updateItems = null;
  }

  function focusItem(menu, index = 0) {
    const items = [...menu.querySelectorAll('button:not(:disabled)')];
    items.at(index)?.focus({ preventScroll: true });
  }

  function openSubmenu(button, item) {
    if (trigger === button) {
      focusItem(submenu);
      return;
    }
    closeSubmenu();
    trigger = button;
    button.setAttribute('aria-expanded', 'true');
    submenu = document.createElement('div');
    submenu.className = 'context-menu';
    submenu.setAttribute('role', 'menu');
    submenu.setAttribute('aria-label', item.label);
    fill(submenu, item.items);
    document.body.append(submenu);
    const rect = button.getBoundingClientRect();
    const panel = root.getBoundingClientRect();
    const right = panel.right + 4;
    const x = right + submenu.offsetWidth + 6 <= innerWidth ? right : panel.left - submenu.offsetWidth - 4;
    position(submenu, x, rect.top - 4);
    focusItem(submenu);
  }

  function fill(menu, items) {
    menu.replaceChildren();
    for (const item of items) {
      const button = document.createElement('button');
      button.type = 'button';
      button.tabIndex = -1;
      button.setAttribute('role', item.checked == null ? 'menuitem' : 'menuitemradio');
      if (item.checked != null) button.setAttribute('aria-checked', item.checked);
      if (item.items) {
        button.setAttribute('aria-haspopup', 'menu');
        button.setAttribute('aria-expanded', 'false');
      }
      button.disabled = !!item.disabled;
      button.title = item.hint || '';
      button.classList.toggle('destructive', !!item.danger);
      button.classList.toggle('separated', !!item.separator);
      button.textContent = item.label;
      if (item.items || item.checked) {
        const mark = document.createElement('span');
        mark.setAttribute('aria-hidden', 'true');
        mark.textContent = item.items ? '›' : '✓';
        button.append(mark);
      }
      button.addEventListener('click', () => {
        if (item.items) {
          openSubmenu(button, item);
        } else {
          close(true);
          runAction(item.action);
        }
      });
      menu.append(button);
    }
  }

  function open(event, row, selector, items) {
    event.preventDefault();
    close();
    restoreFocus = () => {
      const target = row.isConnected ? row : selector && document.querySelector(selector);
      if (target) target.focus({ preventScroll: true });
    };
    const rect = row.getBoundingClientRect();
    const keyboard = event.clientX === 0 && event.clientY === 0;
    const x = keyboard ? rect.left + 8 : event.clientX;
    const y = keyboard ? rect.bottom : event.clientY;
    const place = () => position(root, x, y);
    const update = (nextItems) => {
      if (updateItems !== update) return;
      const focusedIndex = [...root.children].indexOf(document.activeElement);
      closeSubmenu();
      fill(root, nextItems);
      place();
      const focused = root.children[focusedIndex];
      if (focused && !focused.disabled) focused.focus({ preventScroll: true });
      else focusItem(root);
      return true;
    };
    updateItems = update;
    root.hidden = false;
    update(items);
    return update;
  }

  document.addEventListener('contextmenu', (event) => {
    if (event.defaultPrevented) return;
    close();
    if (event.target.closest('#q')) return;
    const selection = window.getSelection();
    const selected = selection?.toString();
    if (selected && selection.containsNode(event.target, true)) {
      open(event, event.target, '', [{ label: 'Copy', action: () => copyText(selected) }]);
    } else {
      event.preventDefault();
    }
  });
  document.addEventListener('pointerdown', (event) => {
    if (!event.target.closest('.context-menu')) close();
  });
  document.addEventListener('scroll', (event) => {
    if (!event.target.closest?.('.context-menu')) close();
  }, true);
  window.addEventListener('blur', () => close());
  window.addEventListener('resize', () => close());
  document.addEventListener('keydown', (event) => {
    if (root.hidden) return;
    const active = document.activeElement;
    const menu = active.closest('.context-menu');
    if (event.key === 'Tab') {
      close(true);
      return;
    }
    if (event.key === 'Escape') {
      if (submenu) {
        trigger.focus({ preventScroll: true });
        closeSubmenu();
      } else close(true);
    } else if (!menu) {
      return;
    } else if (event.key === 'ArrowLeft' && menu === submenu) {
      trigger.focus({ preventScroll: true });
      closeSubmenu();
    } else if (event.key === 'ArrowRight' && active.hasAttribute('aria-haspopup')) {
      active.click();
    } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      const items = [...menu.querySelectorAll('button:not(:disabled)')];
      const index = items.indexOf(active);
      const step = event.key === 'ArrowDown' ? 1 : -1;
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? -1 : (index + step + items.length) % items.length;
      items.at(next)?.focus();
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }, true);

  return { open, close };
})();
