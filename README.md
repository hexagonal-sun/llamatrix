# llamatrix - An ollama Matrix bridge bot

Llamatrix allows you to chat with a LLM through the Matrix protocol. It bridges
ollama and Matrix together, allowing you to showcase LLM in public rooms or chat
to them personally.

## Screenshot

![Alt text](/screenshots/chat.png?raw=true "Chat Screenshot")

## Usage

To get up and running you'll need:

  - A matrix bot account that the llamatrix will log into.
  - An ollama server with a model already downloaded.

You can then run llamatrix with the following command-line

``` shell
llamatrix --username <matrix-user-account> --password <accounts-password> --model <ollama model to use>
```

By default the homserver is set to `matrix.org` and the ollama url is
`http://localhost:11434`. You can override them with the `--homeserver` and
`--url` parameters, respectively.

Once your bot is up and running open up a DM and send a message. The bot will
then accept the invite and begin processing your message with ollama. If you
invite the bot to a public room, it will accept the invite, but it will only
respond to prompts that are prefixed with `!llama`.
