#define   DOUBLE(x)   ( (x)*2 )
#define MULTI(a,b) \
	a+b

/@
	"Summary: a doc comment   "
	"Name: keep(  me  )"
@/
function keep(  me  )
{
	s = "  spaces ,inside( strings )  ";
	t = &"MENU_KEEP   THIS";
	/*  block   comment,(kept)
	    second line   */
	x=1;   // trailing   comment
	y=2;		/* aligned */
// column zero comment
		// over-indented comment
	/#
		println( "dev" ,x ) ;
	      iprintln(x);
	#/
#if   DEVELOPER
	z=3;
		w = DOUBLE(  2  ) ;
#else
	z=4 ;
#endif
	MY_MACRO( a,b );
	v=5;
}
