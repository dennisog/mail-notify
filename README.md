# mail-notify

This is me trying to learn Rust...

This is be a small daemon designed to run as an Emacs subprocess using
[prodigy](https://github.com/rejeep/prodigy.el). All it does is to sit and keep
an IDLE connection open to a mail server. When there is new incoming mail, it
does the following things (in that order):

1. launches [mbsync](https://isync.sourceforge.io/mbsync.html) to synchronize
   all mail to the local Maildir
2. finds the most recently downloaded email in the Maildir, parses it, and
   sends a desktop notification containing its sender and subject
3. plays a wav file embedded into the binary (TODO)
4. calls a hardcoded dbus method (code TBD) that prompts emacs to re-index my
   mail using [mu](https://github.com/djcb/mu) and refresh any open mu4e
   windows (TODO)

The config is all done using environment variables +
[envconfig-rs](https://github.com/greyblake/envconfig-rs).

This is all somewhat of a rube-goldberg machine but I am intending to replace a
system in which the IDLE listening and 1) is done in a node.js program, 2) is
done in bash, and 3) and 4) are done in Python. So, from that perspective, this
is actually less complex than what it is replacing.

However, first and foremost, this is just me learning Rust. Some of the logic
is inspired by [buzz](https://github.com/jonhoo/buzz), which does something
very similar, only a little less specific to my needs.
