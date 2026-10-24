"""Stop a Firefox that Marionette started and leave nothing of it in TMPDIR (task 69a4).

m.quit() alone stops Firefox but keeps its profile (about 50 MB a start). m.cleanup() removes the
profile, but GeckoInstance.close(clean=True) then sets `profile = None`, whose setter makes a new
blank profile (user.js and an empty prefs.js) with mkdtemp that nothing deletes.
"""


def close(m):
    """Quit the Firefox `m` drives, then delete its profile and the blank one Marionette made after.

    Call it once, at the end of a run, from a `finally`; it never raises. After it `m` cannot start
    Firefox again.
    """
    try:
        m.quit()  # in-app first, so Firefox shuts down as it would for a user
    except Exception:
        pass
    try:
        m.cleanup()  # stops Firefox if it is still up, removes the profile, and marks `m` cleaned up
    except Exception:
        pass
    try:
        blank = m.instance.profile if m.instance else None
        if blank is not None:
            blank.cleanup()  # created by Marionette (create_new), so this removes the directory
    except Exception:
        pass
