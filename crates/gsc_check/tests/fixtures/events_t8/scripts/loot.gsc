// Self-written fixture for the event rules under BO4 semantics: waittill returns the payload
// struct and takes several events.
#namespace loot;

function drop()
{
    self notify( "picked_up", { #item:self.item, #count:1 } );
    level notify( "round_over" );
    level notify( "match_over", { #winner:self } );
}

function pickup()
{
    s = self waittill( "picked_up" );
    item = s.item;
    who = s.player;
    event = s._notify;
    s.extra = 1;
    extra = s.extra;
    r = level waittill( "round_over", "match_over" );
    winner = r.winner;
    loser = r.loser;
}

function branches( flag )
{
    if ( flag )
    {
        r = level waittill( "round_over" );
    }
    else
    {
        r = level waittill( "match_over" );
    }
    winner = r.winner;
}

function computed( ev )
{
    r = level waittill( "picked_up", ev );
    anything = r.anything;
}

function matched()
{
    r = self waittillmatch( { #item:"gun" }, "picked_up" );
    count = r.count;
}
