# Autofill test pages

Pages for trying the password manager and form autofill by hand:

```bash
python tests/extensions/server.py 8766 tests/autofill
```

- `http://127.0.0.1:8766/login.html`: a sign-in form. **Type a test sign-in and submit** stands in for typing, so HakuCX offers to save the login. Back on the page, click into the email field to see the saved account and fill it.
- `http://127.0.0.1:8766/checkout.html`: address and payment card forms. Add an address and a card under Settings → Autofill and passwords first. `127.0.0.1` counts as a secure page, so cards are offered here.

Every page shows its fields' values, so you can see what was filled.
