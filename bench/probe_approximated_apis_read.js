// The probe file for firefox_probe.py that returns probe_approximated_apis.js's counts (GAPS B15)
const log = window.wrappedJSObject.__plog;
return log ? JSON.parse(JSON.stringify(log)) : null;
