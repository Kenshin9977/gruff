// Self-written fixture for V001 (hash equality).
#namespace hashes;

function compare( name )
{
    if ( name == #"spawned" )
    {
        return 1;
    }
    if ( ( #"goaway" ) == #"GoAway" )
    {
        return 2;
    }
    if ( name != #"spawned" )
    {
        return 3;
    }
    if ( name == "spawned" )
    {
        return 4;
    }
    return 0;
}
