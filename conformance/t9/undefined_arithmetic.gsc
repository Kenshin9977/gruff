// question: what does undefined + 1 do at run time: an error that ends the thread, or a value?
// fact: undefined + 1 is a non-terminal script error (code #9ad8e396): the thread goes on and the result is undefined; the error reporter calls Com_Error only for terminal errors
// vs t8: T8: the reporter always ends in Com_Error(0x20), which aborts in the dump's session state
// evidence: static: ScrVm_Error (0x1bb06e0) calls the reporter (0x1bb78e0), whose Com_Error call (0x56463a0) is on the terminal path only (test bl at 0x1bb7c5c), then longjmps into VM_Execute's recovery
// expect: f0 T0 before
// expect: f0 T0 script error: #9ad8e396
// expect: f0 T0 after 0
#namespace zz_test;

function main()
{
	iprintln("before");
	x = undefined;
	y = x + 1;
	iprintln("after " + isdefined(y));
}
