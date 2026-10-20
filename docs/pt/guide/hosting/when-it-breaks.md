# Quando algo quebra

Os problemas que as pessoas realmente encontram, na ordem em que costumam acontecer.
Para cada um: o que você vê, por quê, e o que fazer.

::: tip A primeira coisa a tentar, sempre
Se a instalação parou, **execute a mesma linha de novo**.
Ela lembra o que já funcionou e continua de onde parou.
Ela nunca faz a mesma pergunta duas vezes.
:::

## Durante a instalação

### "The name … does not lead anywhere yet"

**Por quê:** seu endereço (por exemplo `app.example.com`) ainda não está conectado à sua máquina.

1. Entre na empresa onde você comprou o endereço.
2. Abra a página de **DNS** dela (às vezes chamada de **Zona** ou **Registros DNS**).
3. Adicione um registro exatamente com o que a mensagem exibiu:

   | Tipo | Nome | Valor |
   |---|---|---|
   | `A` | a palavra que a mensagem mostra (`@` para o endereço puro) | os números que a mensagem mostra |

4. Salve.
5. Espere cinco minutos.
6. Execute a mesma linha de novo.

::: details O campo "Nome" é o erro mais comum
Para `example.com`, digite `@` (alguns sites querem o campo vazio).
Para `app.example.com`, digite apenas `app`, não o endereço completo.
:::

### "The name … does not lead to this machine"

**Por quê:** o endereço aponta para outro lugar: uma máquina antiga, ou uma página de estacionamento da empresa que o vendeu.

1. Abra a mesma página de **DNS**.
2. Apague todos os outros registros `A` com esse nome.
3. Mantenha apenas o que tem o valor que a mensagem exibiu.
4. Espere cinco minutos e execute a mesma linha de novo.

Se você mudou isso há poucos minutos, responda **sim** quando o instalador oferecer esperar.
Ele verifica a cada 20 segundos por até 10 minutos.

### "The address https://… is not answering yet"

**Por quê:** quase sempre uma de duas coisas.

- Você apontou o endereço para a máquina há apenas alguns minutos. **Espere dez minutos** e execute a mesma linha de novo.
- Seu provedor tem um firewall próprio na frente da máquina, e ele está fechado.

Para abrir o firewall do provedor:

1. Abra o painel de controle do seu provedor.
2. Encontre as configurações de **Firewall** ou **Segurança** da máquina.
3. Permita **TCP 80** e **TCP 443** de entrada.
4. Execute a mesma linha de novo.

### "Something on this machine is already answering on port 80 / 443"

**Por quê:** seu provedor instalou um servidor web na máquina para você. Ele ocupa o lugar de que o OpenTraderWorld precisa.

Cole isto e execute a mesma linha de novo:

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" or "Only … MB of disk space is free"

**Por quê:** a máquina é pequena demais. O OpenTraderWorld precisa de cerca de **2 GB de memória** e **8 GB de disco livre**.

1. No seu provedor, redimensione a máquina para um plano maior.
2. Execute a mesma linha de novo.

### "This installer only knows Ubuntu and Debian"

**Por quê:** a máquina foi criada com outro sistema.

1. No seu provedor, **reinstale** (ou **reconstrua**) a máquina com **Ubuntu 24.04** ou **Debian 13**.
2. Execute a mesma linha de novo.

### "This needs the machine's administrator rights"

**Por quê:** você está conectado com uma conta que não tem permissão para instalar software.

Execute a linha de novo com `sudo` no meio, exatamente como a mensagem mostra:

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## Depois da instalação

### A página não abre mais

1. Entre na sua máquina.
2. Digite:

   ```bash
   otw status
   ```

3. Se disser **Nothing is running** ou **is not answering**, digite:

   ```bash
   otw restart
   ```

4. Espere um minuto e recarregue a página.

### Não consigo mais entrar na própria máquina

**Por quê:** o instalador reforçou a forma como a máquina deixa as pessoas entrarem.

- Se você escolheu **a chave**: entre pelo mesmo computador que usou no dia da instalação. Senhas são recusadas de propósito.
- Se você escolheu **uma senha**: entre com o nome de conta mostrado no seu cartão, **não** `root`. O login direto como `root` é recusado de propósito.

Perdeu esse computador ou essa senha? Use o **console** (às vezes chamado de **VNC**, **resgate** ou **terminal web**) no painel de controle do seu provedor. Ele funciona mesmo quando o acesso normal está fechado.

### Esqueci a senha do OpenTraderWorld

1. Entre na sua máquina.
2. Digite (troque `admin` pelo seu nome de login, se o alterou):

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Entre no app com a senha que ele exibir. O app pedirá que você escolha uma nova.

Para ver seu endereço e nome de login de novo, digite `otw card`.

### O navegador mostra "Não seguro" ou recusa a página

- Digite o endereço com `https://` na frente.
- Se começou alguns minutos após a instalação, espere dez minutos: o cadeado ainda está sendo emitido.
- Em uma instalação doméstica (não em um servidor alugado), veja [Solução de problemas](/pt/guide/troubleshooting).

### A máquina está cheia

**Por quê:** os backups noturnos e o histórico de preços baixado ocupam espaço com o tempo.

1. Digite `otw status` para ver quanto espaço sobrou.
2. No seu provedor, dê à máquina um disco maior.
3. Digite `otw restart`.

## Ainda travado?

1. Entre na sua máquina.
2. Digite:

   ```bash
   otw report
   ```

3. Ele grava um arquivo e mostra onde está. O arquivo não contém nenhuma senha.
4. Abra uma issue no [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), diga o que você estava fazendo e anexe esse arquivo.
