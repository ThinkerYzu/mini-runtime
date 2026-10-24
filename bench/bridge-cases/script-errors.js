// script-errors.html's external script (see there): loaded once by the parser and once by script


L((document.currentScript.src.endsWith("?injected") ? "injected" : "parsed") + " external place", J(place(new Error("w"))));
L("external stack", J(frames(nested())));
