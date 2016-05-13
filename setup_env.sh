echo "Setting up development env"
NEW_PS1="(micro2)$PS1"
# Setup customized bash evn
env PS1="$NEW_PS1" PATH=`npm bin`:$PATH bash --norc -i
