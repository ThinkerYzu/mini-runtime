"""The owner's input as WebDriver actions, for the bridge cases that need it and for the end-to-end
check on X (INPUT_ACTIONS=FILE): done by firefox_probe.py in Firefox's own tab and by mini_realsites.py
in the bridge tab, so both pages get the same trusted events from Firefox. FILE is a JSON list of
steps, each a list:

  ["move", SELECTOR]          the mouse to the centre of the element (the first match wholly in the
                              viewport, for "click" too)
  ["click", SELECTOR]         move there, press and release the left button
  ["down"] / ["up"]           press or release the left button where the mouse is
  ["type", TEXT]              a key press and release for each character (WebDriver's key values
                              for Tab, Enter and the others: "\\ue004", "\\ue007")
  ["wheel", SELECTOR, DX, DY] a wheel turn of DX, DY CSS pixels over the element
  ["wheelat", X, Y, DX, DY]   a wheel turn over the point X, Y of the viewport (a page whose
                              elements are taller than the viewport, as X's timeline)
  ["pause", MS]               wait between steps
  ["waitfor", SELECTOR]       wait until the element is there (["waitfor", SELECTOR, MS]: fail after MS)
  ["waiturl", TEXT]           wait until the tab's URL contains TEXT (["waiturl", TEXT, MS]: fail after MS)
  ["hide"]                    another tab opened and selected, so this one is hidden (sent to the
                              background, as when the owner switches tabs); WebDriver commands still
                              go to this tab
  ["show"]                    this tab selected again, the other one closed
  ["mark", LABEL]             a checkpoint: the driver's `on_mark(label)` runs there (the end-to-end run
                              on X takes the memory of both sides and a screenshot at each)
  ["closewindow"]             the browser window closed, as by the owner's click on its X: Firefox
                              quits with it (the last step; the driver's session ends)
"""
import time

from marionette_driver.by import By

MOUSE = {"pointerType": "mouse"}


def find(m, selector):
    try:
        return m.find_element(By.CSS_SELECTOR, selector)
    except Exception:
        return None


# the first element of a selector that is wholly inside the viewport and not covered at its centre
# (X's sticky header lies over the scrolled timeline), else the first one
IN_VIEW = """const all = [...document.querySelectorAll(arguments[0])];
return all.find(e => { const r = e.getBoundingClientRect();
  if (!(r.height > 0 && r.top >= 0 && r.left >= 0 && r.bottom <= window.innerHeight && r.right <= window.innerWidth)) return false;
  const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  return !!hit && (hit === e || e.contains(hit)); }) || all[0] || null;"""


def find_in_view(m, selector):
    """For a click or a move: after a scroll the first match can be above the viewport, where a
    WebDriver pointer cannot go (X's timeline keeps the posts scrolled past)."""
    try:
        return m.execute_script(IN_VIEW, script_args=(selector,), sandbox="actions")
    except Exception:
        return None


class Steps:
    """Steps done a few at a time: advance() does those that are due and returns, so a driver that
    serves a runtime in between (mini_realsites.py's wait) keeps the page running while a pause or a
    waitfor holds. `done` counts the steps done; `failed` names a step whose element was missing or
    whose wait timed out. `on_mark(label)` runs at each mark, and `marks` keeps when each was reached."""

    def __init__(self, steps, on_mark=None):
        self.steps = steps
        self.done = 0
        self.until = None
        self.failed = None
        self.on_mark = on_mark
        self.marks = []
        self.other_tab = None

    def finished(self):
        return self.done >= len(self.steps) or self.failed is not None

    def advance(self, m):
        while not self.finished():
            step = self.steps[self.done]
            kind, args = step[0], step[1:]
            if kind == "pause":
                if self.until is None:
                    self.until = time.time() + args[0] / 1000
                if time.time() < self.until:
                    return
                self.until = None
            elif kind in ("waitfor", "waiturl"):
                if self.until is None and len(args) > 1:
                    self.until = time.time() + args[1] / 1000
                if (find(m, args[0]) is None) if kind == "waitfor" else (args[0] not in m.get_url()):
                    if self.until is not None and time.time() >= self.until:
                        self.failed = step
                    return
                self.until = None
            elif kind == "mark":
                self.marks.append([args[0], time.time()])
                if self.on_mark:
                    self.on_mark(args[0])
            elif kind == "hide":
                mine = m.current_window_handle
                self.other_tab = m.open(type="tab", focus=True)["handle"]
                m.switch_to_window(mine, focus=False)
            elif kind == "show":
                mine = m.current_window_handle
                if self.other_tab is not None:
                    m.switch_to_window(self.other_tab, focus=False)
                    m.close()
                    self.other_tab = None
                m.switch_to_window(mine, focus=True)
            elif kind == "closewindow":
                # counted as done first: the session may end before the call returns
                self.done += 1
                with m.using_context("chrome"):
                    m.execute_script("window.setTimeout(() => window.close(), 0);")
                return
            elif kind == "type":
                m.actions.sequence("key", "keyboard").send_keys(args[0]).perform()
            elif kind in ("down", "up"):
                seq = m.actions.sequence("pointer", "mouse", MOUSE)
                (seq.pointer_down() if kind == "down" else seq.pointer_up()).perform()
            elif kind == "wheelat":
                m.actions.sequence("wheel", "wheel").scroll(args[0], args[1], args[2], args[3], origin="viewport").perform()
            elif kind in ("move", "click", "wheel"):
                element = find_in_view(m, args[0]) if kind != "wheel" else find(m, args[0])
                if element is None:
                    self.failed = step
                    return
                if kind == "wheel":
                    m.actions.sequence("wheel", "wheel").scroll(0, 0, args[1], args[2], origin=element).perform()
                else:
                    seq = m.actions.sequence("pointer", "mouse", MOUSE)
                    (seq.pointer_move(0, 0, origin=element) if kind == "move" else seq.click(element=element)).perform()
            else:
                raise ValueError(f"input_actions: unknown step {step!r}")
            self.done += 1


def perform(m, steps, on_mark=None):
    """Does `steps` in the current tab of `m`, waiting through the pauses; returns the Steps."""
    s = Steps(steps, on_mark)
    while not s.finished():
        s.advance(m)
        time.sleep(0.05)
    return s
