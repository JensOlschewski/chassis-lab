function design = readDesign(file)
% readDesign reads a flat design JSON {name: number or text} into a dictionary.
s = jsondecode(fileread(file));
names = string(fieldnames(s));
design = configureDictionary("string", "cell");
for n = names'
    value = s.(n);
    if ~ischar(value)
        value = double(value);
    end
    design(n) = {value};
end
end
